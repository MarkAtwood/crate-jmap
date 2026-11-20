//! Mailbox sharing in the memory backend (draft-ietf-jmap-mail-sharing-02,
//! RFC 9670 §4): who the caller is in an account, the caller's view of a
//! Mailbox, the rights checks of Mailbox writes, and the per-user change
//! log entries a Mailbox change makes.
//!
//! The rules are those of the module documentation, "Caller context and
//! rights".

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use jmap_mail_types::{Email, Mailbox, MailboxRights};
use jmap_server::{BackendSetError, SetError, SetErrorType};
use jmap_types::{Id, JmapObject};
use serde_json::{Map, Value};

use super::{ChangeEntry, Inner, MemoryError};

/// The RFC 8621 §2 MailboxRights properties (rfc8621.txt:627-689).
const RFC8621_RIGHTS: [&str; 9] = [
    "mayReadItems",
    "mayAddItems",
    "mayRemoveItems",
    "maySetSeen",
    "maySetKeywords",
    "mayCreateChild",
    "mayRename",
    "mayDelete",
    "maySubmit",
];

/// The Mailbox properties a sharee needs `mayRename` to change: the RFC
/// 8621 §2 right covers `name` and `parentId`; this backend applies it to
/// the other presentation properties a sharee could otherwise rewrite for
/// every user too.
const RENAME_PROPERTIES: [&str; 6] = [
    "name",
    "parentId",
    "role",
    "sortOrder",
    "locale",
    "localizations",
];

/// Who a caller is in an account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Access {
    /// The account owner, or a caller without a principal.
    Owner,
    /// Any other Principal.
    Sharee(Id),
}

/// The users a change-log entry is reported to.
#[derive(Clone, Debug)]
pub(super) enum Audience {
    /// The account owner (and callers without a principal), and these
    /// sharees. A non-Mailbox entry names no sharee: a sharee reads no
    /// other type.
    Owner(BTreeSet<Id>),
    /// One sharee only: a change of what that user can see, or of that
    /// user's own `isSubscribed`.
    Sharee(Id),
}

impl Default for Audience {
    fn default() -> Self {
        Self::Owner(BTreeSet::new())
    }
}

impl Audience {
    /// Whether the entry is reported to the sharee `sharee`, or to the
    /// owner when `None`.
    pub(super) fn includes(&self, sharee: Option<&Id>) -> bool {
        match (self, sharee) {
            (Self::Owner(_), None) => true,
            (Self::Owner(users), Some(u)) => users.contains(u),
            (Self::Sharee(_), None) => false,
            (Self::Sharee(only), Some(u)) => only == u,
        }
    }
}

/// The kind of a Mailbox change, for [`Inner::log_mailbox_change`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MailboxChange {
    Created,
    Updated,
    Destroyed,
    Restored,
}

/// Each sharee of an account, with the Mailbox ids that user can see.
pub(super) type Visibility = HashMap<Id, HashSet<Id>>;

fn set_error(t: SetErrorType) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(SetError::new(t)))
}

fn invalid(property: &str) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::InvalidProperties).with_properties([property]),
    ))
}

/// The root property a patch path writes (`shareWith/bob` → `shareWith`).
fn root(path: &str) -> &str {
    path.split_once('/').map_or(path, |(r, _)| r)
}

/// Whether `rights` grants `right`.
fn grants(rights: &Value, right: &str) -> bool {
    rights.get(right).and_then(Value::as_bool) == Some(true)
}

/// The `parentId` of a stored Mailbox.
fn parent_of(mailbox: &Value) -> Option<Id> {
    mailbox
        .get("parentId")
        .and_then(Value::as_str)
        .map(Id::from)
}

impl Inner {
    /// The Principal that owns `account`: the one
    /// `MemoryBackend::with_account_owner` named, else the account id.
    pub(super) fn owner_of(&self, account: &str) -> Id {
        self.account_owners
            .get(account)
            .cloned()
            .unwrap_or_else(|| Id::from(account))
    }

    /// Whether `principal` acts as the owner of `account`.
    pub(super) fn is_owner(&self, account: &str, principal: Option<&Id>) -> bool {
        principal.is_none_or(|p| *p == self.owner_of(account))
    }

    /// Who `principal` is in `account`.
    pub(super) fn access(&self, account: &str, principal: Option<&Id>) -> Access {
        match principal {
            Some(p) if !self.is_owner(account, Some(p)) => Access::Sharee(p.clone()),
            _ => Access::Owner,
        }
    }

    /// The Mailbox `myRights` of the account owner: every RFC 8621 §2
    /// right and `mayShare` true (draft-ietf-jmap-mail-sharing-02 §3.6:
    /// "implicit full rights"). `maySetMetadata` is present, and true, only
    /// when `with_metadata_info` lists Mailbox or Email (-05 §4.1: the
    /// right exists for a type the capability lists "or a type it
    /// contains").
    fn owner_mailbox_rights(&self) -> Value {
        let mut rights: Map<String, Value> = RFC8621_RIGHTS
            .into_iter()
            .map(|k| (k.to_owned(), Value::Bool(true)))
            .collect();
        rights.insert("mayShare".to_owned(), Value::Bool(true));
        if self.metadata_on() {
            rights.insert("maySetMetadata".to_owned(), Value::Bool(true));
        }
        Value::Object(rights)
    }

    /// Whether the Mailbox rights carry `maySetMetadata`.
    fn metadata_on(&self) -> bool {
        self.metadata_info.contains_key(Mailbox::TYPE_NAME)
            || self.metadata_info.contains_key(Email::TYPE_NAME)
    }

    /// A sharee's rights from its `shareWith` entry (`None`: no entry,
    /// every right false), with every right present: an absent right is
    /// not granted.
    fn sharee_rights(&self, entry: Option<&Value>) -> Value {
        let get = |k: &str| Value::Bool(entry.is_some_and(|e| grants(e, k)));
        let mut rights: Map<String, Value> = RFC8621_RIGHTS
            .into_iter()
            .map(|k| (k.to_owned(), get(k)))
            .collect();
        rights.insert("mayShare".to_owned(), get("mayShare"));
        if self.metadata_on() {
            rights.insert("maySetMetadata".to_owned(), get("maySetMetadata"));
        }
        Value::Object(rights)
    }

    /// The Mailboxes of `account` that sharee `user` can see: those whose
    /// `shareWith` names the user, and their ancestors, which the user sees
    /// without rights (RFC 8621 §2 mayReadItems: "If a sub-Mailbox is
    /// shared but not the parent Mailbox, this may be false").
    fn visible_to(&self, account: &str, user: &Id) -> HashSet<Id> {
        let mut visible = HashSet::new();
        let Some(mailboxes) = self.objects_ref(Mailbox::TYPE_NAME, account) else {
            return visible;
        };
        for (id, m) in mailboxes {
            if m.get("shareWith")
                .and_then(Value::as_object)
                .is_some_and(|sw| sw.contains_key(user.as_ref()))
            {
                let mut next = Some(id.clone());
                // Bounded: a parent chain has at most one step per Mailbox.
                for _ in 0..=mailboxes.len() {
                    let Some(cur) = next.take() else { break };
                    if !visible.insert(cur.clone()) {
                        break;
                    }
                    next = mailboxes.get(&cur).and_then(parent_of);
                }
            }
        }
        visible
    }

    /// Every user named in a `shareWith` of `account`, with what each can
    /// see.
    pub(super) fn sharee_visibility(&self, account: &str) -> Visibility {
        let users: BTreeSet<Id> = self
            .objects_ref(Mailbox::TYPE_NAME, account)
            .into_iter()
            .flat_map(HashMap::values)
            .filter_map(|m| m.get("shareWith").and_then(Value::as_object))
            .flat_map(|sw| sw.keys().map(|k| Id::from(k.as_str())))
            .collect();
        users
            .into_iter()
            .map(|u| {
                let v = self.visible_to(account, &u);
                (u, v)
            })
            .collect()
    }

    /// The view of `account` for `principal`.
    pub(super) fn viewer<'a>(&'a self, account: &'a str, principal: Option<&Id>) -> Viewer<'a> {
        let access = self.access(account, principal);
        let visible = match &access {
            Access::Owner => HashSet::new(),
            Access::Sharee(u) => self.visible_to(account, u),
        };
        Viewer {
            inner: self,
            account,
            access,
            visible,
        }
    }

    /// Check a `shareWith` value about to be stored on a Mailbox of
    /// `account`, and normalize it: it must decode as
    /// `Id[MailboxRights]|null` and must not name the owner
    /// (draft-ietf-jmap-mail-sharing-02 §2: "The Principal to which this
    /// mailbox belongs MUST NOT be in the map"); an empty map is stored as
    /// `null` ("null if it is not shared with anyone").
    pub(super) fn check_share_with(
        &self,
        account: &str,
        mailbox: &mut Map<String, Value>,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let Some(value) = mailbox.get("shareWith") else {
            return Ok(());
        };
        let decoded: Option<BTreeMap<Id, MailboxRights>> =
            serde_json::from_value(value.clone()).map_err(|_| invalid("shareWith"))?;
        match decoded {
            Some(map) if map.contains_key(&self.owner_of(account)) => Err(invalid("shareWith")),
            Some(map) if !map.is_empty() => Ok(()),
            _ => {
                mailbox.insert("shareWith".to_owned(), Value::Null);
                Ok(())
            }
        }
    }

    /// The rights checks of a Mailbox create in `account` by `principal`,
    /// under the store lock, and the creator's `isSubscribed` when the
    /// creator is a sharee. `val` is the Mailbox to store; a sharee's
    /// create inherits the parent's `shareWith` when it gives none.
    pub(super) fn check_mailbox_create(
        &self,
        account: &str,
        principal: Option<&Id>,
        val: &mut Value,
    ) -> Result<Option<(Id, bool)>, BackendSetError<MemoryError>> {
        let Value::Object(obj) = val else {
            return Ok(None);
        };
        let viewer = self.viewer(account, principal);
        let subscription = if let Access::Sharee(user) = &viewer.access {
            // RFC 8621 §1.3.1: only the owner may create at the top level
            // (mayCreateTopLevelMailbox).
            let parent_id = obj
                .get("parentId")
                .and_then(Value::as_str)
                .map(Id::from)
                .ok_or_else(|| set_error(SetErrorType::Forbidden))?;
            let parent = self
                .objects_ref(Mailbox::TYPE_NAME, account)
                .and_then(|m| m.get(&parent_id))
                .filter(|_| viewer.visible.contains(&parent_id))
                .ok_or_else(|| invalid("parentId"))?;
            let rights = viewer.rights(parent).unwrap_or(Value::Null);
            // A role is the account's: one Mailbox per role (RFC 8621 §2),
            // checked against Mailboxes a sharee may not see.
            let gives_role = obj.get("role").is_some_and(|v| !v.is_null());
            if !grants(&rights, "mayCreateChild") || gives_role {
                return Err(set_error(SetErrorType::Forbidden));
            }
            let gives_share_with = obj.get("shareWith").is_some_and(|v| !v.is_null());
            if gives_share_with && !grants(&rights, "mayShare") {
                return Err(set_error(SetErrorType::Forbidden));
            }
            if !gives_share_with {
                // The sharee keeps the access it had to the parent.
                let inherited = parent.get("shareWith").cloned().unwrap_or(Value::Null);
                obj.insert("shareWith".to_owned(), inherited);
            }
            // The stored `isSubscribed` is the owner's.
            let subscribed = obj
                .insert("isSubscribed".to_owned(), Value::Bool(false))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some((user.clone(), subscribed))
        } else {
            None
        };
        self.check_share_with(account, obj)?;
        Ok(subscription)
    }

    /// The rights checks of an update of Mailbox `stored` in `account` by
    /// `principal` with `patch`, under the store lock. A sharee's
    /// `isSubscribed` is taken out of `patch` and returned, to be stored
    /// as that user's own.
    pub(super) fn check_mailbox_update(
        &self,
        account: &str,
        principal: Option<&Id>,
        stored: &Value,
        patch: &mut Map<String, Value>,
    ) -> Result<Option<(Id, bool)>, BackendSetError<MemoryError>> {
        let viewer = self.viewer(account, principal);
        let Access::Sharee(user) = &viewer.access else {
            return Ok(None);
        };
        // mail-sharing §3.1 and §3.4: a Mailbox the user cannot see does
        // not exist.
        let rights = viewer
            .rights(stored)
            .ok_or_else(|| set_error(SetErrorType::NotFound))?;
        let subscription = match patch.remove("isSubscribed") {
            None => None,
            Some(Value::Bool(b)) => Some((user.clone(), b)),
            Some(_) => return Err(invalid("isSubscribed")),
        };
        for key in patch.keys() {
            let needed = match root(key) {
                "shareWith" => "mayShare",
                p if RENAME_PROPERTIES.contains(&p) => "mayRename",
                _ => continue,
            };
            if !grants(&rights, needed) {
                return Err(set_error(SetErrorType::Forbidden));
            }
        }
        if let Some(parent) = patch.get("parentId") {
            // RFC 8621 §1.3.1 and §2 mayCreateChild, as for a create.
            let parent_id = parent
                .as_str()
                .map(Id::from)
                .ok_or_else(|| set_error(SetErrorType::Forbidden))?;
            let new_parent = self
                .objects_ref(Mailbox::TYPE_NAME, account)
                .and_then(|m| m.get(&parent_id))
                .filter(|_| viewer.visible.contains(&parent_id))
                .ok_or_else(|| invalid("parentId"))?;
            if !viewer
                .rights(new_parent)
                .is_some_and(|r| grants(&r, "mayCreateChild"))
            {
                return Err(set_error(SetErrorType::Forbidden));
            }
        }
        Ok(subscription)
    }

    /// The rights checks of a destroy of Mailbox `id` in `account` by
    /// `principal`, under the store lock: a Mailbox the user cannot see is
    /// `notFound`, one without `mayDelete` `forbidden`, and one that still
    /// holds Email `mailboxHasEmail` (RFC 8621 §2.5), whoever asks.
    pub(super) fn check_mailbox_destroy(
        &self,
        account: &str,
        principal: Option<&Id>,
        id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let stored = self
            .objects_ref(Mailbox::TYPE_NAME, account)
            .and_then(|m| m.get(id))
            .ok_or_else(|| set_error(SetErrorType::NotFound))?;
        let rights = self
            .viewer(account, principal)
            .rights(stored)
            .ok_or_else(|| set_error(SetErrorType::NotFound))?;
        if !grants(&rights, "mayDelete") {
            return Err(set_error(SetErrorType::Forbidden));
        }
        let holds_email = self
            .objects_ref(Email::TYPE_NAME, account)
            .into_iter()
            .flat_map(HashMap::values)
            .any(|e| {
                e.get("mailboxIds")
                    .and_then(Value::as_object)
                    .is_some_and(|m| m.get(id.as_ref()) == Some(&Value::Bool(true)))
            });
        if holds_email {
            return Err(set_error(SetErrorType::MailboxHasEmail));
        }
        Ok(())
    }

    /// Record a Mailbox change of `id` in `account` at a new state, for the
    /// users who can see it: the owner, and the sharees who see `id` after
    /// a create or restore, before a destroy, and both before and after an
    /// update. Every Mailbox a sharee gains or loses sight of (`before` is
    /// [`Self::sharee_visibility`] before the change) is reported to that
    /// sharee alone as created or destroyed (RFC 8620 §5.2 for that user).
    /// With `only`, the change is that sharee's own and is reported to
    /// nobody else.
    pub(super) fn log_mailbox_change(
        &mut self,
        account: &str,
        change: MailboxChange,
        id: &Id,
        updated_props: Option<BTreeSet<String>>,
        before: &Visibility,
        only: Option<&Id>,
    ) {
        let after = self.sharee_visibility(account);
        let new_state = self.bump_state(Mailbox::TYPE_NAME, account);
        let entry = |audience: Audience,
                     created: Vec<Id>,
                     updated: Vec<Id>,
                     destroyed: Vec<Id>,
                     restored: Vec<Id>| ChangeEntry {
            new_state,
            created,
            updated,
            destroyed,
            restored,
            updated_props: None,
            audience,
        };
        let empty = HashSet::new();
        let sees = |vis: &Visibility, u: &Id| vis.get(u).unwrap_or(&empty).contains(id);
        let users: BTreeSet<Id> = before.keys().chain(after.keys()).cloned().collect();
        let mut entries = Vec::new();
        if let Some(user) = only {
            entries.push(entry(
                Audience::Sharee(user.clone()),
                vec![],
                vec![id.clone()],
                vec![],
                vec![],
            ));
        } else {
            let audience: BTreeSet<Id> = users
                .iter()
                .filter(|u| match change {
                    MailboxChange::Created | MailboxChange::Restored => sees(&after, u),
                    MailboxChange::Destroyed => sees(before, u),
                    MailboxChange::Updated => sees(before, u) && sees(&after, u),
                })
                .cloned()
                .collect();
            let one = vec![id.clone()];
            let mut main = match change {
                MailboxChange::Created => {
                    entry(Audience::Owner(audience), one, vec![], vec![], vec![])
                }
                MailboxChange::Updated => {
                    entry(Audience::Owner(audience), vec![], one, vec![], vec![])
                }
                MailboxChange::Destroyed => {
                    entry(Audience::Owner(audience), vec![], vec![], one, vec![])
                }
                MailboxChange::Restored => {
                    entry(Audience::Owner(audience), vec![], vec![], vec![], one)
                }
            };
            main.updated_props = updated_props;
            entries.push(main);
        }
        for u in &users {
            let b = before.get(u).unwrap_or(&empty);
            let a = after.get(u).unwrap_or(&empty);
            let mut gained: Vec<Id> = a.difference(b).cloned().collect();
            let mut lost: Vec<Id> = b.difference(a).cloned().collect();
            // The main entry already reports `id` to a user who sees it.
            match change {
                MailboxChange::Created | MailboxChange::Restored => gained.retain(|m| m != id),
                MailboxChange::Destroyed => lost.retain(|m| m != id),
                MailboxChange::Updated => {}
            }
            gained.sort();
            lost.sort();
            if !gained.is_empty() {
                entries.push(entry(
                    Audience::Sharee(u.clone()),
                    gained,
                    vec![],
                    vec![],
                    vec![],
                ));
            }
            if !lost.is_empty() {
                entries.push(entry(
                    Audience::Sharee(u.clone()),
                    vec![],
                    vec![],
                    lost,
                    vec![],
                ));
            }
        }
        self.change_log_mut(Mailbox::TYPE_NAME, account)
            .extend(entries);
    }
}

/// A caller's view of one account.
pub(super) struct Viewer<'a> {
    inner: &'a Inner,
    account: &'a str,
    pub(super) access: Access,
    /// For a sharee, the Mailboxes it can see.
    visible: HashSet<Id>,
}

impl Viewer<'_> {
    /// The sharee this view is for, or `None` for the owner.
    pub(super) fn sharee(&self) -> Option<&Id> {
        match &self.access {
            Access::Owner => None,
            Access::Sharee(u) => Some(u),
        }
    }

    /// The caller's `myRights` on a stored Mailbox, or `None` when the
    /// caller cannot see it.
    pub(super) fn rights(&self, stored: &Value) -> Option<Value> {
        match &self.access {
            Access::Owner => Some(self.inner.owner_mailbox_rights()),
            Access::Sharee(u) => {
                let id = stored.get("id").and_then(Value::as_str)?;
                if !self.visible.contains(id) {
                    return None;
                }
                let entry = stored.get("shareWith").and_then(|sw| sw.get(u.as_ref()));
                Some(self.inner.sharee_rights(entry))
            }
        }
    }

    /// The caller's view of a stored `type_name` object, or `None` when the
    /// caller cannot see it. A Mailbox has the caller's `myRights` (never
    /// the stored value, RFC 8621 §2) and `isSubscribed`, and for a sharee
    /// without `mayShare` a `shareWith` of the sharee's own entry only
    /// (mail-sharing §3.4). A sharee sees no other type.
    pub(super) fn view(&self, type_name: &str, stored: &Value) -> Option<Value> {
        if type_name != Mailbox::TYPE_NAME {
            return match self.access {
                Access::Owner => Some(stored.clone()),
                Access::Sharee(_) => None,
            };
        }
        let rights = self.rights(stored)?;
        let mut view = stored.clone();
        let Value::Object(m) = &mut view else {
            return Some(view);
        };
        if let Access::Sharee(u) = &self.access {
            let id = m.get("id").and_then(Value::as_str).map(Id::from)?;
            let subscribed = self
                .inner
                .subscriptions
                .get(&(self.account.to_owned(), id, u.clone()))
                .copied()
                // RFC 8621 §2: "This SHOULD default to false for Mailboxes
                // in shared accounts the user has access to".
                .unwrap_or(false);
            m.insert("isSubscribed".to_owned(), Value::Bool(subscribed));
            if !grants(&rights, "mayShare") {
                let own = m
                    .get("shareWith")
                    .and_then(|sw| sw.get(u.as_ref()))
                    .cloned()
                    .map(|e| Value::Object(Map::from_iter([(u.to_string(), e)])))
                    .unwrap_or(Value::Null);
                m.insert("shareWith".to_owned(), own);
            }
        }
        m.insert("myRights".to_owned(), rights);
        Some(view)
    }
}
