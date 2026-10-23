//! Typed vocabulary enums: `ChatPermission`, `ChannelPermissionTargetType`
//! and `EphemeralDataType`.
//!
//! Oracle: wire strings quoted from the drafts at jmap-chat-spec 6bcd9d8
//! and hand-written JSON frames.

use jmap_chat_types::{
    COMPANION_EPHEMERAL_DATA_TYPES, SPEC_CHANNEL_PERMISSION_TARGET_TYPES,
    SPEC_EPHEMERAL_DATA_TYPES, SPEC_PERMISSION_NAMES,
};
use jmap_chat_types::{
    ChannelPermission, ChannelPermissionTargetType, ChatPermission, ChatStreamEnable,
    EphemeralDataType, RolePatch, SpaceRole,
};
use serde_json::json;

/// draft-atwood-jmap-chat-00 L797-807: "Defined permission names:
/// `"view"` [...] `"send"` [...] `"pin"` [...] `"manage_channels"` [...]
/// `"manage_members"` [...] `"manage_roles"` [...] `"manage_space"` [...]
/// `"ban"` [...] `"mention_broadcast"` [...] `"start_call"`".
#[test]
fn spec_permission_names_map_to_known_variants() {
    let cases = [
        ("view", ChatPermission::View),
        ("send", ChatPermission::Send),
        ("pin", ChatPermission::Pin),
        ("manage_channels", ChatPermission::ManageChannels),
        ("manage_members", ChatPermission::ManageMembers),
        ("manage_roles", ChatPermission::ManageRoles),
        ("manage_space", ChatPermission::ManageSpace),
        ("ban", ChatPermission::Ban),
        ("mention_broadcast", ChatPermission::MentionBroadcast),
        ("start_call", ChatPermission::StartCall),
    ];
    for (wire, variant) in cases {
        let parsed: ChatPermission = serde_json::from_value(json!(wire)).unwrap();
        assert_eq!(parsed, variant, "{wire}");
        assert_eq!(parsed.as_str(), wire);
        assert_eq!(serde_json::to_value(&parsed).unwrap(), json!(wire));
    }
}

/// draft-atwood-jmap-chat-00 L809: "Companion specifications MAY register
/// additional permission names. Servers MUST ignore unrecognized
/// permission names." An unknown name must survive a round trip so a
/// server can store and echo it unchanged.
#[test]
fn unknown_permission_round_trips() {
    let raw = json!({
        "id": "r1",
        "name": "Mod",
        "permissions": ["view", "x-acme-archive"],
        "position": 2
    });
    let role: SpaceRole = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(
        role.permissions,
        [
            ChatPermission::View,
            ChatPermission::Other("x-acme-archive".to_owned())
        ]
    );
    assert_eq!(serde_json::to_value(&role).unwrap(), raw);

    let patch: RolePatch =
        serde_json::from_value(json!({"permissions": ["x-acme-archive"]})).unwrap();
    assert_eq!(
        patch.permissions,
        Some(vec![ChatPermission::Other("x-acme-archive".to_owned())])
    );
}

/// draft-atwood-jmap-chat-00 L855-856: "`targetType` (String): `"role"`
/// or `"member"`."
#[test]
fn channel_permission_target_types() {
    let raw = json!({
        "targetId": "m1",
        "targetType": "member",
        "allow": ["send"],
        "deny": ["pin", "x-acme-react"]
    });
    let p: ChannelPermission = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(p.target_type, ChannelPermissionTargetType::Member);
    assert_eq!(p.allow, [ChatPermission::Send]);
    assert_eq!(
        p.deny,
        [
            ChatPermission::Pin,
            ChatPermission::Other("x-acme-react".to_owned())
        ]
    );
    assert_eq!(serde_json::to_value(&p).unwrap(), raw);

    let role: ChannelPermissionTargetType = serde_json::from_value(json!("role")).unwrap();
    assert_eq!(role, ChannelPermissionTargetType::Role);
    let other: ChannelPermissionTargetType = serde_json::from_value(json!("group")).unwrap();
    assert_eq!(
        other,
        ChannelPermissionTargetType::Other("group".to_owned())
    );
    assert_eq!(serde_json::to_value(&other).unwrap(), json!("group"));
}

/// draft-atwood-jmap-chat-wss-00 L175: "This specification defines
/// `"typing"` and `"presence"`. It also defines two companion values:
/// `"vtc"` [...] and `"scene"`". L194 requires unrecognized values that
/// appear alongside recognized ones to be ignored, so they must parse.
#[test]
fn ephemeral_data_types() {
    let raw = json!({
        "@type": "ChatStreamEnable",
        "dataTypes": ["typing", "presence", "vtc", "scene", "x-acme-cursor"]
    });
    let frame: ChatStreamEnable = serde_json::from_value(raw).unwrap();
    assert_eq!(
        frame.data_types,
        [
            EphemeralDataType::Typing,
            EphemeralDataType::Presence,
            EphemeralDataType::Vtc,
            EphemeralDataType::Scene,
            EphemeralDataType::Other("x-acme-cursor".to_owned()),
        ]
    );
    assert_eq!(
        serde_json::to_value(&frame).unwrap()["dataTypes"],
        json!(["typing", "presence", "vtc", "scene", "x-acme-cursor"])
    );
}

/// The const wire-string slices in `vocabulary` and the enum variants are
/// two lists of the same spec vocabulary; every const entry must parse to
/// a known variant, never `Other`.
#[test]
fn vocabulary_consts_match_enum_variants() {
    for s in SPEC_PERMISSION_NAMES {
        let p: ChatPermission = serde_json::from_value(json!(s)).unwrap();
        assert!(!matches!(p, ChatPermission::Other(_)), "{s}");
    }
    for s in SPEC_CHANNEL_PERMISSION_TARGET_TYPES {
        let t: ChannelPermissionTargetType = serde_json::from_value(json!(s)).unwrap();
        assert!(!matches!(t, ChannelPermissionTargetType::Other(_)), "{s}");
    }
    for s in SPEC_EPHEMERAL_DATA_TYPES
        .iter()
        .chain(COMPANION_EPHEMERAL_DATA_TYPES)
    {
        let d: EphemeralDataType = serde_json::from_value(json!(s)).unwrap();
        assert!(!matches!(d, EphemeralDataType::Other(_)), "{s}");
    }
}
