//! The protobuf Wire format is standard protobuf: prost, a protobuf library
//! that knows nothing of serde, reads what this crate encodes, and this crate
//! reads what prost encodes, which leaves out every field at its default.

use std::fmt::Debug;

use kotodama_protocol::WireFormat;
use prost::Message;
use serde::Serialize;
use serde::de::DeserializeOwned;

#[path = "support/messages.rs"]
mod messages;
#[path = "support/mirror.rs"]
#[expect(dead_code, reason = "only the prost copies are read back here")]
mod mirror;

fn both_ways<T, P>(ours: &T, theirs: &P)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
    P: Message + Default + PartialEq,
{
    let encoded = WireFormat::Protobuf.encode(ours).unwrap();
    let read_by_prost =
        P::decode(encoded.as_slice()).expect("prost reads the protobuf Wire format");
    assert!(read_by_prost == *theirs, "prost read something else");

    let from_prost: T = WireFormat::Protobuf
        .decode(&theirs.encode_to_vec())
        .expect("the protobuf Wire format reads prost");
    assert_eq!(from_prost, *ours);
}

#[test]
fn a_query_reads_the_same_in_prost() {
    let query = messages::query();
    both_ways(&query, &mirror::prost::query(&query));
}

#[test]
fn results_read_the_same_in_prost() {
    let results = messages::results();
    both_ways(&results, &mirror::prost::results(&results));
}

#[test]
fn a_batch_of_items_reads_the_same_in_prost() {
    let batch = messages::batch();
    both_ways(&batch, &mirror::prost::batch(&batch));
}
