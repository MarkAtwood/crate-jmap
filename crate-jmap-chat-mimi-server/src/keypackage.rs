//! draft-atwood-jmap-chat-mimi-00 §KeyPackage/get and §KeyPackage/set.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use jmap_chat_mimi_types::{Base64Url, KeyPackage, KeyPackageCreate, MimiCapability};
use jmap_server::{
    enforce_max_objects_in_set, extract_account_id, server_fail_from_backend,
    server_fail_value_from_backend, HandlerOutput,
};
use jmap_types::{Id, JmapError};
use openmls::ciphersuite::hash_ref::make_key_package_ref;
use openmls::component::{ComponentId, ComponentType};
use openmls::prelude::{
    BasicCredential, CredentialType, Extension, KeyPackageIn, KeyPackageVerifyError, OpenMlsCrypto,
    ProtocolVersion,
};
use serde_json::{json, Map, Value};
use tls_codec::{Deserialize as _, Serialize as _};

use crate::backend::{BackendSetError, MimiBackend, NewKeyPackage, SetError, SetErrorType};

/// An `invalidProperties` SetError naming `property`.
fn invalid(property: &str, description: &str) -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties([property])
        .with_description(description)
}

/// Check one `KeyPackage/set` create against
/// draft-atwood-jmap-chat-mimi-00 §KeyPackage/set and §KeyPackage Object
/// Fields.
///
/// "On create, the server MUST reject the record with `invalidProperties`
/// naming `keyPackage` if `keyPackage` does not parse as a KeyPackage, if
/// its signature does not verify, if its cipher suite is not in
/// `cipherSuites`, if its leaf node lifetime has expired, if its credential
/// is not a Basic credential, or if the credential identity is not
/// `userUri`." `lastResort` is derived from the KeyPackage's
/// `app_data_dictionary` extension: `true` when it holds a
/// `last_resort_key_package` component (draft-ietf-mls-extensions-10 §6.4);
/// a create whose `lastResort` differs is refused with `invalidProperties`
/// naming `lastResort`.
///
/// "Does not parse" covers trailing octets and a KeyPackage wrapped in an
/// MLSMessage: `keyPackage` is the bare RFC 9420 §10 structure. "Signature
/// does not verify" covers every RFC 9420 §10.1 validity failure openmls
/// reports: the leaf node and KeyPackage signatures, the protocol version,
/// an `init_key` equal to the encryption key, and an extension missing from
/// the leaf node capabilities. openmls checks the lifetime against the
/// system clock, and also rejects a lifetime whose `not_before` is in the
/// future.
///
/// The KeyPackageRef is computed over the exact `keyPackage` octets. The
/// account-wide rules (signature-key binding, KeyPackageRef reuse,
/// `maxKeyPackages`, last-resort replacement) are
/// [`MimiBackend::create_key_package`]'s.
// SetError is the type every `/set` refusal takes; boxing it buys nothing.
#[allow(clippy::result_large_err)]
pub fn check_key_package_create<C: OpenMlsCrypto>(
    crypto: &C,
    capability: &MimiCapability,
    create: &KeyPackageCreate,
) -> Result<NewKeyPackage, SetError> {
    let not_a_key_package = || invalid("keyPackage", "keyPackage does not parse as a KeyPackage");
    let bytes = URL_SAFE_NO_PAD
        .decode(create.key_package.as_str())
        .map_err(|_| not_a_key_package())?;
    let key_package_in =
        KeyPackageIn::tls_deserialize_exact(&bytes).map_err(|_| not_a_key_package())?;
    let key_package = key_package_in
        .validate(crypto, ProtocolVersion::Mls10)
        .map_err(|e| match e {
            KeyPackageVerifyError::LifetimeError(_) => invalid(
                "keyPackage",
                "keyPackage leaf node lifetime is not valid now",
            ),
            _ => invalid("keyPackage", "keyPackage is not a valid KeyPackage"),
        })?;

    let ciphersuite = key_package.ciphersuite();
    if !capability.cipher_suites.contains(&u16::from(ciphersuite)) {
        return Err(invalid(
            "keyPackage",
            "keyPackage cipher suite is not in cipherSuites",
        ));
    }

    let credential = key_package.leaf_node().credential();
    if credential.credential_type() != CredentialType::Basic {
        return Err(invalid(
            "keyPackage",
            "keyPackage credential is not a Basic credential",
        ));
    }
    let basic = BasicCredential::try_from(credential.clone()).map_err(|_| not_a_key_package())?;
    if basic.identity() != capability.user_uri.as_bytes() {
        return Err(invalid(
            "keyPackage",
            "keyPackage credential identity is not userUri",
        ));
    }

    let last_resort_component = ComponentId::from(ComponentType::LastResortKeyPackage);
    let last_resort = key_package.extensions().iter().any(|ext| {
        matches!(ext, Extension::AppDataDictionary(dict)
            if dict.dictionary().contains(&last_resort_component))
    });
    if create.last_resort.is_some_and(|claim| claim != last_resort) {
        return Err(invalid(
            "lastResort",
            "lastResort differs from the value derived from keyPackage",
        ));
    }

    let key_package_ref = make_key_package_ref(&bytes, ciphersuite, crypto)
        .ok()
        .and_then(|r| r.tls_serialize_detached().ok())
        .ok_or_else(|| {
            SetError::new(SetErrorType::custom("serverFail"))
                .with_description("KeyPackageRef computation failed")
        })?;
    let key_package_ref = Base64Url::try_from(URL_SAFE_NO_PAD.encode(key_package_ref))
        .expect("base64 URL_SAFE_NO_PAD output is canonical unpadded base64url");

    Ok(NewKeyPackage {
        key_package: create.key_package.clone(),
        key_package_ref,
        last_resort,
        signature_key: key_package.leaf_node().signature_key().as_slice().to_vec(),
    })
}

/// Decode one `create` value: `keyPackage` is required, `lastResort` is
/// optional, and any other property is refused (RFC 8620 §5.3: a create
/// naming a server-set or unknown property is `invalidProperties`).
#[allow(clippy::result_large_err)]
fn decode_create(value: Value) -> Result<KeyPackageCreate, SetError> {
    let Value::Object(map) = value else {
        return Err(SetError::new(SetErrorType::InvalidProperties)
            .with_description("a create must be a KeyPackage object"));
    };
    if !matches!(map.get("keyPackage"), Some(Value::String(s)) if s.parse::<Base64Url>().is_ok()) {
        return Err(invalid(
            "keyPackage",
            "keyPackage must be an unpadded base64url string",
        ));
    }
    if !matches!(
        map.get("lastResort"),
        None | Some(Value::Null | Value::Bool(_))
    ) {
        return Err(invalid("lastResort", "lastResort must be a Boolean"));
    }
    let unknown: Vec<&str> = map
        .keys()
        .map(String::as_str)
        .filter(|k| !matches!(*k, "keyPackage" | "lastResort"))
        .collect();
    if !unknown.is_empty() {
        return Err(SetError::new(SetErrorType::InvalidProperties)
            .with_properties(unknown)
            .with_description("only keyPackage and lastResort may be set on create"));
    }
    serde_json::from_value(Value::Object(map)).map_err(|_| {
        SetError::new(SetErrorType::InvalidProperties).with_description("invalid KeyPackage")
    })
}

/// The SetError JSON for a backend `/set` failure; storage errors become
/// `serverFail` without their `Display` text.
fn backend_set_error_value<E: std::fmt::Display>(e: BackendSetError<E>) -> Value {
    match e {
        BackendSetError::SetError(se) => set_error_value(&se),
        BackendSetError::Other(e) => server_fail_value_from_backend(&e),
        _ => json!({
            "type": "serverFail",
            "description": "unhandled backend error variant",
        }),
    }
}

/// Serialize a [`SetError`] for a `notCreated` / `notUpdated` /
/// `notDestroyed` map.
fn set_error_value(e: &SetError) -> Value {
    serde_json::to_value(e).expect("derive(Serialize) on plain data is infallible")
}

/// Handle `KeyPackage/get` (draft-atwood-jmap-chat-mimi-00 §KeyPackage/get):
/// "Standard JMAP `/get` (RFC 8620 §5.1)".
pub async fn handle_key_package_get<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_get::<KeyPackage, B>(backend, caller, args).await
}

/// Handle `KeyPackage/set` (draft-atwood-jmap-chat-mimi-00 §KeyPackage/set).
///
/// Standard RFC 8620 §5.3 `/set`, "except that `update` is not supported:
/// the server MUST reject every update with a SetError of type
/// `forbidden`". Create runs [`check_key_package_create`], then
/// [`MimiBackend::create_key_package`]; destroy calls
/// [`MimiBackend::destroy_key_package`].
pub async fn handle_key_package_set<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: an unknown accountId is `accountNotFound`.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<KeyPackage>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(Value::as_str) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut created = Map::new();
    let mut not_created = Map::new();
    let mut not_updated = Map::new();
    let mut destroyed = Vec::new();
    let mut not_destroyed = Map::new();
    let mut mutated = false;

    if let Some(Value::Object(create_map)) = args.remove("create") {
        let capability = backend
            .mimi_capability(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        for (create_id, obj) in create_map {
            let checked = match decode_create(obj) {
                Ok(create) => check_key_package_create(backend.mls_crypto(), &capability, &create)
                    .map(|new| (create.last_resort.is_none(), new)),
                Err(e) => Err(e),
            };
            let (echo_last_resort, new) = match checked {
                Ok(c) => c,
                Err(e) => {
                    not_created.insert(create_id, set_error_value(&e));
                    continue;
                }
            };
            let key_package_ref = new.key_package_ref.clone();
            let last_resort = new.last_resort;
            match backend
                .create_key_package(caller, &account_id, new, capability.max_key_packages)
                .await
            {
                Ok(id) => {
                    mutated = true;
                    // RFC 8620 §5.3: `created` holds the server-set
                    // properties and any the client did not send.
                    let mut entry = json!({ "id": id, "keyPackageRef": key_package_ref });
                    if echo_last_resort {
                        entry["lastResort"] = Value::Bool(last_resort);
                    }
                    created.insert(create_id, entry);
                }
                Err(e) => {
                    not_created.insert(create_id, backend_set_error_value(e));
                }
            }
        }
    }

    if let Some(Value::Object(update_map)) = args.remove("update") {
        let forbidden = set_error_value(
            &SetError::new(SetErrorType::Forbidden)
                .with_description("KeyPackage records cannot be updated"),
        );
        for (id, _) in update_map {
            not_updated.insert(id, forbidden.clone());
        }
    }

    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of `destroy` is an Id.
        let ids: Vec<&str> = destroy_arr
            .iter()
            .map(|v| v.as_str().ok_or(v))
            .collect::<Result<_, _>>()
            .map_err(|bad| {
                JmapError::invalid_arguments(format!(
                    "destroy: every element must be a string Id; got {bad}"
                ))
            })?;
        for id in ids {
            match backend
                .destroy_key_package(caller, &account_id, &Id::from(id))
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed.push(Value::String(id.to_owned()));
                }
                Err(e) => {
                    not_destroyed.insert(id.to_owned(), backend_set_error_value(e));
                }
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<KeyPackage>(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
    } else {
        old_state.clone()
    };
    let map_or_null = |m: Map<String, Value>| {
        if m.is_empty() {
            Value::Null
        } else {
            Value::Object(m)
        }
    };

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": map_or_null(created),
        "updated": Value::Null,
        "destroyed": if destroyed.is_empty() { Value::Null } else { Value::Array(destroyed) },
        "notCreated": map_or_null(not_created),
        "notUpdated": map_or_null(not_updated),
        "notDestroyed": map_or_null(not_destroyed),
    })))
}
