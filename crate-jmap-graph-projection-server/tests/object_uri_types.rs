//! Every `Projectable::URI_TYPE` is registered in the "JMAP Object URI
//! Types" registry for its data type (draft-atwood-jmap-graph-projection-00
//! §Which Objects Are Projected: "a type name registered in the 'JMAP
//! Object URI Types' registry"), as collected in `jmap-object-uri`.

use jmap_contacts_types::{AddressBook, ContactCard};
use jmap_graph_projection_server::Projectable;
use jmap_mail_types::{Email, EmailSubmission, Identity, Mailbox, Thread};
use jmap_object_uri::uri_type;
use jmap_types::JmapObject;

fn check<T: Projectable + JmapObject>() {
    assert_eq!(
        uri_type(T::URI_TYPE).map(|e| e.data_type),
        Some(T::TYPE_NAME),
        "{}",
        T::URI_TYPE
    );
}

#[test]
fn uri_types_are_registered_for_their_data_types() {
    check::<Mailbox>();
    check::<Thread>();
    check::<Email>();
    check::<Identity>();
    check::<EmailSubmission>();
    check::<AddressBook>();
    check::<ContactCard>();
}
