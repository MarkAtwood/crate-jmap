//! `Delegate*` companions of the chat hook traits: implement one to forward a whole hook
//! trait to another backend (docs/design/chat-backend-split.md §3.2).
//!
//! A delegated hook trait runs with the target as `self`. When a default of one hook trait
//! calls another (the [`ChatModeration`] defaults write through [`ChatStore`]; the
//! [`ChatStore`] defaults report through [`ChatEvents`]), a delegated trait reaches the
//! target's implementation, not the wrapper's. A wrapper that needs its own behaviour under a
//! delegated trait implements that trait itself.
#![allow(clippy::too_many_arguments)]

use super::*;

/// Forward every [`ChatStore`] method to [`chat_store`](Self::chat_store)'s target.
///
/// Implement this instead of [`ChatStore`]; a type implements one or the other, never both.
pub trait DelegateChatStore: JmapBackend {
    /// The backend that answers the [`ChatStore`] methods.
    type Target: ChatStore<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    /// The target, usually the wrapped backend.
    fn chat_store(&self) -> &Self::Target;
}

impl<T: DelegateChatStore + ChatEvents> ChatStore for T {
    fn create_object<O, F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        build: F,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
    + Send
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&jmap_types::Id) -> O + Send,
    {
        self.chat_store()
            .create_object::<O, F>(caller, account_id, create_id, build)
    }

    fn create_message<F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        build: F,
    ) -> impl std::future::Future<
        Output = Result<(jmap_types::Id, jmap_chat_types::Message), BackendSetError<Self::Error>>,
    > + Send
    where
        F: FnOnce(&jmap_types::Id) -> jmap_chat_types::Message + Send,
    {
        self.chat_store()
            .create_message::<F>(caller, account_id, create_id, build)
    }

    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send
    {
        self.chat_store()
            .update_object::<O>(caller, account_id, id, patch)
    }

    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send {
        self.chat_store()
            .destroy_object::<O>(caller, account_id, id)
    }

    fn supports_type<O: JmapObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> bool {
        self.chat_store().supports_type::<O>(caller, account_id)
    }
}

/// Forward every [`ChatSpaces`] method to [`chat_spaces`](Self::chat_spaces)'s target.
///
/// Implement this instead of [`ChatSpaces`]; a type implements one or the other, never both.
pub trait DelegateChatSpaces: JmapBackend {
    /// The backend that answers the [`ChatSpaces`] methods.
    type Target: ChatSpaces<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    /// The target, usually the wrapped backend.
    fn chat_spaces(&self) -> &Self::Target;
}

impl<T: DelegateChatSpaces> ChatSpaces for T {
    fn generate_invite_code(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<String, Self::Error>> + Send {
        self.chat_spaces().generate_invite_code(caller, account_id)
    }

    fn join_space_with_invite(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        invite_id: &jmap_types::Id,
        joined_at: &jmap_types::UTCDate,
    ) -> impl std::future::Future<Output = Result<Result<(), InviteJoinRefusal>, Self::Error>> + Send
    {
        self.chat_spaces()
            .join_space_with_invite(caller, account_id, invite_id, joined_at)
    }

    fn apply_space_patch(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        space_id: &jmap_types::Id,
        ops: Vec<SpacePatchOp>,
    ) -> impl std::future::Future<Output = Result<Vec<OpResult>, BackendSetError<Self::Error>>> + Send
    {
        self.chat_spaces()
            .apply_space_patch(caller, account_id, space_id, ops)
    }

    fn apply_space_metadata_patch(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        space_id: &jmap_types::Id,
        patch: jmap_chat_types::SpaceMetadataPatch,
    ) -> impl std::future::Future<
        Output = Result<Option<jmap_chat_types::Space>, BackendSetError<Self::Error>>,
    > + Send {
        self.chat_spaces()
            .apply_space_metadata_patch(caller, account_id, space_id, patch)
    }

    fn space_extension_properties(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> &'static [&'static str] {
        self.chat_spaces()
            .space_extension_properties(caller, account_id)
    }

    fn protect_last_admin(&self, caller: &Self::CallerCtx, account_id: &jmap_types::Id) -> bool {
        self.chat_spaces().protect_last_admin(caller, account_id)
    }

    fn everyone_permissions(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        space: &jmap_chat_types::Space,
    ) -> impl std::future::Future<
        Output = Result<std::collections::HashSet<jmap_chat_types::ChatPermission>, Self::Error>,
    > + Send {
        self.chat_spaces()
            .everyone_permissions(caller, account_id, space)
    }
}

/// Forward every [`SpacePolicy`] method to [`space_policy`](Self::space_policy)'s target.
///
/// Implement this instead of [`SpacePolicy`]; a type implements one or the other, never both.
pub trait DelegateSpacePolicy: JmapBackend {
    /// The backend that answers the [`SpacePolicy`] methods.
    type Target: SpacePolicy<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    /// The target, usually the wrapped backend.
    fn space_policy(&self) -> &Self::Target;
}

impl<T: DelegateSpacePolicy> SpacePolicy for T {
    fn may_set_custom_emoji(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _target_space_id: Option<&jmap_types::Id>,
        _op: EmojiSetOp,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.space_policy()
            .may_set_custom_emoji(_caller, _account_id, _target_space_id, _op)
    }

    fn may_set_space_ban(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _space_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.space_policy()
            .may_set_space_ban(_caller, _account_id, _space_id)
    }

    fn may_set_space_invite(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _space_id: &jmap_types::Id,
        _created_by: Option<&jmap_types::Id>,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.space_policy()
            .may_set_space_invite(_caller, _account_id, _space_id, _created_by)
    }

    fn may_get_space_invite(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _invite: &jmap_chat_types::SpaceInvite,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        self.space_policy()
            .may_get_space_invite(_caller, _account_id, _invite)
    }

    fn may_get_space_ban(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _ban: &jmap_chat_types::SpaceBan,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        self.space_policy()
            .may_get_space_ban(_caller, _account_id, _ban)
    }
}

/// Forward every [`ChatAccessPolicy`] method to [`chat_access_policy`](Self::chat_access_policy)'s target.
///
/// Implement this instead of [`ChatAccessPolicy`]; a type implements one or the other, never both.
pub trait DelegateChatAccessPolicy: JmapBackend {
    /// The backend that answers the [`ChatAccessPolicy`] methods.
    type Target: ChatAccessPolicy<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    /// The target, usually the wrapped backend.
    fn chat_access_policy(&self) -> &Self::Target;
}

impl<T: DelegateChatAccessPolicy> ChatAccessPolicy for T {
    fn is_contact_blocked(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        contact_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        self.chat_access_policy()
            .is_contact_blocked(caller, account_id, contact_id)
    }

    fn may_broadcast_mention(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _space_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.chat_access_policy()
            .may_broadcast_mention(_caller, _account_id, _space_id, _chat_id)
    }

    fn may_set_pinned_messages(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.chat_access_policy()
            .may_set_pinned_messages(_caller, _account_id, _chat_id)
    }

    fn may_send_message(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.chat_access_policy()
            .may_send_message(_caller, _account_id, _chat_id)
    }

    fn may_view_chat(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        self.chat_access_policy()
            .may_view_chat(_caller, _account_id, _chat_id)
    }

    fn validate_pinned_message_ids(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        chat_id: &jmap_types::Id,
        added: &[jmap_types::Id],
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.chat_access_policy()
            .validate_pinned_message_ids(caller, account_id, chat_id, added)
    }

    fn slow_mode_check(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), SlowModeError>> + Send {
        self.chat_access_policy()
            .slow_mode_check(_caller, _account_id, _chat_id)
    }
}

/// Forward every [`ChatRestrictions`] method to [`chat_restrictions`](Self::chat_restrictions)'s target.
///
/// Implement this instead of [`ChatRestrictions`]; a type implements one or the other, never both.
pub trait DelegateChatRestrictions: JmapBackend {
    /// The backend that answers the [`ChatRestrictions`] methods.
    type Target: ChatRestrictions<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    /// The target, usually the wrapped backend.
    fn chat_restrictions(&self) -> &Self::Target;
}

impl<T: DelegateChatRestrictions> ChatRestrictions for T {
    fn may_create_message(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat: &jmap_chat_types::Chat,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.chat_restrictions()
            .may_create_message(_caller, _account_id, _chat)
    }

    fn may_update_message(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat: &jmap_chat_types::Chat,
        _message: &jmap_chat_types::Message,
        _patch_keys: &[&str],
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.chat_restrictions().may_update_message(
            _caller,
            _account_id,
            _chat,
            _message,
            _patch_keys,
        )
    }

    fn may_update_chat(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat: &jmap_chat_types::Chat,
        _patch_keys: &[&str],
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.chat_restrictions()
            .may_update_chat(_caller, _account_id, _chat, _patch_keys)
    }
}

/// Forward every [`ChatModeration`] method to [`chat_moderation`](Self::chat_moderation)'s target.
///
/// Implement this instead of [`ChatModeration`]; a type implements one or the other, never both.
pub trait DelegateChatModeration: JmapBackend {
    /// The backend that answers the [`ChatModeration`] methods.
    type Target: ChatModeration<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    /// The target, usually the wrapped backend.
    fn chat_moderation(&self) -> &Self::Target;
}

impl<T: DelegateChatModeration + ChatStore> ChatModeration for T {
    fn create_chat_ban<F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        build: F,
    ) -> impl std::future::Future<
        Output = Result<(jmap_types::Id, jmap_chat_types::ChatBan), BackendSetError<Self::Error>>,
    > + Send
    where
        F: FnOnce(&jmap_types::Id) -> jmap_chat_types::ChatBan + Send,
    {
        self.chat_moderation()
            .create_chat_ban::<F>(caller, account_id, create_id, build)
    }

    fn update_chat_ban(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: jmap_types::PatchObject,
    ) -> impl std::future::Future<
        Output = Result<Option<jmap_chat_types::ChatBan>, BackendSetError<Self::Error>>,
    > + Send {
        self.chat_moderation()
            .update_chat_ban(caller, account_id, id, patch)
    }

    fn destroy_chat_ban(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send {
        self.chat_moderation()
            .destroy_chat_ban(caller, account_id, id)
    }

    fn kick_chat_members(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat: &jmap_chat_types::Chat,
        _user_ids: &[jmap_types::Id],
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        self.chat_moderation()
            .kick_chat_members(_caller, _account_id, _chat, _user_ids)
    }
}

/// Forward every [`ChatMessageLifecycle`] method to [`chat_message_lifecycle`](Self::chat_message_lifecycle)'s target.
///
/// Implement this instead of [`ChatMessageLifecycle`]; a type implements one or the other, never both.
pub trait DelegateChatMessageLifecycle: JmapBackend {
    /// The backend that answers the [`ChatMessageLifecycle`] methods.
    type Target: ChatMessageLifecycle<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    /// The target, usually the wrapped backend.
    fn chat_message_lifecycle(&self) -> &Self::Target;
}

impl<T: DelegateChatMessageLifecycle> ChatMessageLifecycle for T {
    fn read_and_burn_message(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        message_id: &jmap_types::Id,
        patch: jmap_types::PatchObject,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send {
        self.chat_message_lifecycle()
            .read_and_burn_message(caller, account_id, message_id, patch)
    }

    fn expire_message(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _message_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        self.chat_message_lifecycle()
            .expire_message(_caller, _account_id, _message_id)
    }
}

/// Forward every [`ChatEvents`] method to [`chat_events`](Self::chat_events)'s target.
///
/// Implement this instead of [`ChatEvents`]; a type implements one or the other, never both.
pub trait DelegateChatEvents: JmapBackend {
    /// The backend that answers the [`ChatEvents`] methods.
    type Target: ChatEvents<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    /// The target, usually the wrapped backend.
    fn chat_events(&self) -> &Self::Target;
}

impl<T: DelegateChatEvents> ChatEvents for T {
    fn on_chat_destroyed(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = ()> + Send {
        self.chat_events()
            .on_chat_destroyed(caller, account_id, chat_id)
    }

    fn on_space_destroyed(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        space_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = ()> + Send {
        self.chat_events()
            .on_space_destroyed(caller, account_id, space_id)
    }

    fn on_space_ban_created(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        ban: &jmap_chat_types::SpaceBan,
    ) -> impl std::future::Future<Output = ()> + Send {
        self.chat_events()
            .on_space_ban_created(caller, account_id, ban)
    }

    fn on_outbound_event(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        event: &crate::OutboundEvent,
    ) -> impl std::future::Future<Output = ()> + Send {
        self.chat_events()
            .on_outbound_event(caller, account_id, event)
    }
}

/// Forward every [`ChatSettings`] method to [`chat_settings`](Self::chat_settings)'s target.
///
/// Implement this instead of [`ChatSettings`]; a type implements one or the other, never both.
pub trait DelegateChatSettings: JmapBackend {
    /// The backend that answers the [`ChatSettings`] methods.
    type Target: ChatSettings<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    /// The target, usually the wrapped backend.
    fn chat_settings(&self) -> &Self::Target;
}

impl<T: DelegateChatSettings> ChatSettings for T {
    fn limits(&self, _caller: &Self::CallerCtx, _account_id: &jmap_types::Id) -> ChatLimits {
        self.chat_settings().limits(_caller, _account_id)
    }

    fn retains_edit_history(&self) -> bool {
        self.chat_settings().retains_edit_history()
    }

    fn maintains_unread_count(&self) -> bool {
        self.chat_settings().maintains_unread_count()
    }

    fn owner_user_id(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<String>, Self::Error>> + Send {
        self.chat_settings().owner_user_id(_caller, _account_id)
    }

    fn search_capability(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> impl std::future::Future<
        Output = Result<Option<jmap_search::SearchCapability>, Self::Error>,
    > + Send {
        self.chat_settings().search_capability(_caller, _account_id)
    }
}

/// Forward every [`ChatResolvers`] method to [`chat_resolvers`](Self::chat_resolvers)'s target.
///
/// Implement this instead of [`ChatResolvers`]; a type implements one or the other, never both.
pub trait DelegateChatResolvers: JmapBackend {
    /// The backend that answers the [`ChatResolvers`] methods.
    type Target: ChatResolvers<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    /// The target, usually the wrapped backend.
    fn chat_resolvers(&self) -> &Self::Target;
}

impl<T: DelegateChatResolvers> ChatResolvers for T {
    fn attachment_blob(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<
        Output = Result<Result<Option<AttachmentBlob>, SetError>, Self::Error>,
    > + Send {
        self.chat_resolvers()
            .attachment_blob(caller, account_id, blob_id)
    }

    fn filenode_attachment(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat: &jmap_chat_types::Chat,
        _attachment: &jmap_chat_types::AttachmentCreate,
    ) -> impl std::future::Future<
        Output = Result<Result<Option<jmap_chat_types::Attachment>, SetError>, Self::Error>,
    > + Send {
        self.chat_resolvers()
            .filenode_attachment(_caller, _account_id, _chat, _attachment)
    }
}
