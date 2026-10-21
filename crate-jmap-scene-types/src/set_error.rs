//! Scene extensions to the RFC 8620 §5.3 `SetError`
//! (draft-atwood-jmap-scene-00 §Content Moderation for User-Created
//! Objects).
//!
//! The draft adds the `contentPolicy` type. [`jmap_types::SetError`]
//! already carries any type string, so this module only names it.

/// The `SetError` type a server uses for content that fails its content
/// policy: "Servers MAY reject `SceneRegion/set` or `SceneObject/set`
/// create or update requests that violate text or asset content policies
/// with a `SetError` of type `contentPolicy`". "This SetError is only for
/// a rejection made while processing the `/set` call."
pub const CONTENT_POLICY: &str = "contentPolicy";

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_types::SetError;
    use serde_json::json;

    #[test]
    fn content_policy_is_the_draft_type_name() {
        // draft-atwood-jmap-scene-00 (jmap-chat-spec 6bcd9d8) L2688
        // "Servers MAY reject `SceneRegion/set` or `SceneObject/set`
        // create or update requests that violate text or asset content
        // policies with a `SetError` of type `contentPolicy`".
        let err: SetError =
            serde_json::from_value(json!({ "type": "contentPolicy" })).expect("a SetError");
        assert_eq!(err.error_type.to_string(), CONTENT_POLICY);
    }
}
