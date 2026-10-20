//! [`jmap_types::JmapObject`] impls for the VTC data types.
//!
//! Defined here so that `jmap-vtc-server` can use them without violating
//! the orphan rule. The type names are those of draft-atwood-jmap-vtc-00
//! §JMAP Data Type Registration.

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

use crate::query::{
    VtcCallComparator, VtcCallFilter, VtcLivestreamFilter, VtcMediaComparator, VtcRecordingFilter,
};
use crate::{VtcCall, VtcLivestream, VtcParticipant, VtcRecording};

impl JmapObject for VtcCall {
    const TYPE_NAME: &'static str = "VTCCall";
    type Property = ();
}

impl GetObject for VtcCall {}

impl SetObject for VtcCall {
    type Patch = PatchObject;
}

impl QueryObject for VtcCall {
    type Filter = VtcCallFilter;
    type Comparator = VtcCallComparator;
}

impl JmapObject for VtcParticipant {
    const TYPE_NAME: &'static str = "VTCParticipant";
    type Property = ();
}

impl GetObject for VtcParticipant {}

impl JmapObject for VtcRecording {
    const TYPE_NAME: &'static str = "VTCRecording";
    type Property = ();
}

impl GetObject for VtcRecording {}

impl SetObject for VtcRecording {
    type Patch = PatchObject;
}

impl QueryObject for VtcRecording {
    type Filter = VtcRecordingFilter;
    type Comparator = VtcMediaComparator;
}

impl JmapObject for VtcLivestream {
    const TYPE_NAME: &'static str = "VTCLivestream";
    type Property = ();
}

impl GetObject for VtcLivestream {}

impl SetObject for VtcLivestream {
    type Patch = PatchObject;
}

impl QueryObject for VtcLivestream {
    type Filter = VtcLivestreamFilter;
    type Comparator = VtcMediaComparator;
}
