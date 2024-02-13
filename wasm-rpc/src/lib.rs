// Copyright 2024 Golem Cloud
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#[allow(unused)]
#[rustfmt::skip]
#[cfg(feature = "stub")]
mod bindings;

/// A builder interface for WitValue instances
mod builder;

/// Extension methods for extracting values from WitValue instances
mod extractor;

/// Conversion to and from JSON, in the presence of golem-wasm-ast generated type information
#[cfg(feature = "json")]
pub mod json;

/// Protobuf-defined value types and conversion to them
#[cfg(feature = "protobuf")]
pub mod protobuf;

#[cfg(feature = "wasmtime")]
pub mod wasmtime;

use crate::builder::WitValueBuilder;
pub use builder::{NodeBuilder, WitValueBuilderExtensions};
pub use extractor::{WitNodePointer, WitValueExtractor};

#[cfg(feature = "stub")]
pub use bindings::golem::rpc::types::{NodeIndex, Uri, WasmRpc, WitNode, WitValue};

#[cfg(feature = "host")]
use ::wasmtime::component::bindgen;
#[cfg(feature = "host")]
bindgen!({
    path: "wit",
    interfaces: "
      import golem:rpc/types@0.1.0;
    ",
    tracing: false,
    async: true,
    with: {
    }
});

#[cfg(feature = "host")]
pub use golem::rpc::types::{HostWasmRpc, NodeIndex, WitNode, WitValue};

/// A tree representation of Value - isomorphic to the protobuf Val type but easier to work with in Rust
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum Value {
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    S8(i8),
    S16(i16),
    S32(i32),
    S64(i64),
    F32(f32),
    F64(f64),
    Char(char),
    String(String),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    Record(Vec<Value>),
    Variant {
        case_idx: u32,
        case_value: Option<Box<Value>>,
    },
    Enum(u32),
    Flags(Vec<bool>),
    Option(Option<Box<Value>>),
    Result(Result<Option<Box<Value>>, Option<Box<Value>>>),
}

impl From<Value> for WitValue {
    fn from(value: Value) -> Self {
        let mut builder = WitValueBuilder::new();
        build_wit_value(value, &mut builder);
        builder.build()
    }
}

fn build_wit_value(value: Value, builder: &mut WitValueBuilder) -> NodeIndex {
    match value {
        Value::Bool(value) => builder.add_bool(value),
        Value::U8(value) => builder.add_u8(value),
        Value::U16(value) => builder.add_u16(value),
        Value::U32(value) => builder.add_u32(value),
        Value::U64(value) => builder.add_u64(value),
        Value::S8(value) => builder.add_s8(value),
        Value::S16(value) => builder.add_s16(value),
        Value::S32(value) => builder.add_s32(value),
        Value::S64(value) => builder.add_s64(value),
        Value::F32(value) => builder.add_f32(value),
        Value::F64(value) => builder.add_f64(value),
        Value::Char(value) => builder.add_char(value),
        Value::String(value) => builder.add_string(&value),
        Value::List(values) => {
            let list_idx = builder.add_list();
            let mut items = Vec::new();
            for value in values {
                let item_idx = build_wit_value(value, builder);
                items.push(item_idx);
            }
            builder.finish_seq(items, list_idx);
            list_idx
        }
        Value::Tuple(values) => {
            let tuple_idx = builder.add_tuple();
            let mut items = Vec::new();
            for value in values {
                let item_idx = build_wit_value(value, builder);
                items.push(item_idx);
            }
            builder.finish_seq(items, tuple_idx);
            tuple_idx
        }
        Value::Record(fields) => {
            let record_idx = builder.add_record();
            let mut items = Vec::new();
            for value in fields {
                let item_idx = build_wit_value(value, builder);
                items.push(item_idx);
            }
            builder.finish_seq(items, record_idx);
            record_idx
        }
        Value::Variant {
            case_idx,
            case_value: Some(case_value),
        } => {
            let variant_idx = builder.add_variant(case_idx, -1);
            let inner_idx = build_wit_value(*case_value, builder);
            builder.finish_child(inner_idx, variant_idx);
            variant_idx
        }
        Value::Variant {
            case_idx,
            case_value: None,
        } => builder.add_variant_unit(case_idx),
        Value::Enum(value) => builder.add_enum_value(value),
        Value::Flags(values) => builder.add_flags(values),
        Value::Option(value) => {
            if let Some(value) = value {
                let option_idx = builder.add_option_some();
                let inner_idx = build_wit_value(*value, builder);
                builder.finish_child(inner_idx, option_idx);
                option_idx
            } else {
                builder.add_option_none()
            }
        }
        Value::Result(result) => match result {
            Ok(Some(ok)) => {
                let result_idx = builder.add_result_ok();
                let inner_idx = build_wit_value(*ok, builder);
                builder.finish_child(inner_idx, result_idx);
                result_idx
            }
            Ok(None) => builder.add_result_ok_unit(),
            Err(Some(err)) => {
                let result_idx = builder.add_result_err();
                let inner_idx = build_wit_value(*err, builder);
                builder.finish_child(inner_idx, result_idx);
                result_idx
            }
            Err(None) => builder.add_result_err_unit(),
        },
    }
}

impl From<WitValue> for Value {
    fn from(value: WitValue) -> Self {
        assert!(!value.nodes.is_empty());
        build_tree(&value.nodes[0], &value.nodes)
    }
}

fn build_tree(node: &WitNode, nodes: &[WitNode]) -> Value {
    match node {
        WitNode::RecordValue(field_indices) => {
            let mut fields = Vec::new();
            for index in field_indices {
                let value = build_tree(&nodes[*index as usize], nodes);
                fields.push(value);
            }
            Value::Record(fields)
        }
        WitNode::VariantValue((case_idx, Some(inner_idx))) => {
            let value = build_tree(&nodes[*inner_idx as usize], nodes);
            Value::Variant {
                case_idx: *case_idx,
                case_value: Some(Box::new(value)),
            }
        }
        WitNode::VariantValue((case_idx, None)) => Value::Variant {
            case_idx: *case_idx,
            case_value: None,
        },
        WitNode::EnumValue(value) => Value::Enum(*value),
        WitNode::FlagsValue(values) => Value::Flags(values.clone()),
        WitNode::TupleValue(indices) => {
            let mut values = Vec::new();
            for index in indices {
                let value = build_tree(&nodes[*index as usize], nodes);
                values.push(value);
            }
            Value::Tuple(values)
        }
        WitNode::ListValue(indices) => {
            let mut values = Vec::new();
            for index in indices {
                let value = build_tree(&nodes[*index as usize], nodes);
                values.push(value);
            }
            Value::List(values)
        }
        WitNode::OptionValue(Some(index)) => {
            let value = build_tree(&nodes[*index as usize], nodes);
            Value::Option(Some(Box::new(value)))
        }
        WitNode::OptionValue(None) => Value::Option(None),
        WitNode::ResultValue(Ok(Some(index))) => {
            let value = build_tree(&nodes[*index as usize], nodes);
            Value::Result(Ok(Some(Box::new(value))))
        }
        WitNode::ResultValue(Ok(None)) => Value::Result(Ok(None)),
        WitNode::ResultValue(Err(Some(index))) => {
            let value = build_tree(&nodes[*index as usize], nodes);
            Value::Result(Err(Some(Box::new(value))))
        }
        WitNode::ResultValue(Err(None)) => Value::Result(Err(None)),
        WitNode::PrimU8(value) => Value::U8(*value),
        WitNode::PrimU16(value) => Value::U16(*value),
        WitNode::PrimU32(value) => Value::U32(*value),
        WitNode::PrimU64(value) => Value::U64(*value),
        WitNode::PrimS8(value) => Value::S8(*value),
        WitNode::PrimS16(value) => Value::S16(*value),
        WitNode::PrimS32(value) => Value::S32(*value),
        WitNode::PrimS64(value) => Value::S64(*value),
        WitNode::PrimFloat32(value) => Value::F32(*value),
        WitNode::PrimFloat64(value) => Value::F64(*value),
        WitNode::PrimChar(value) => Value::Char(*value),
        WitNode::PrimBool(value) => Value::Bool(*value),
        WitNode::PrimString(value) => Value::String(value.clone()),
    }
}

#[cfg(feature = "arbitrary")]
impl<'a> arbitrary::Arbitrary<'a> for WitValue {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let arbitrary_value = u.arbitrary::<Value>()?;
        Ok(arbitrary_value.into())
    }
}

pub const WASM_RPC_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use crate::{Value, WitValue};
    use proptest::prelude::*;
    use proptest_arbitrary_interop::arb_sized;

    const CASES: u32 = 10000;
    const SIZE: usize = 4096;

    proptest! {

        #![proptest_config(ProptestConfig {
            cases: CASES, .. ProptestConfig::default()
        })]
        #[test]
        fn round_trip(value in arb_sized::<Value>(SIZE).prop_filter("Value must be equal to itself", |v| v.eq(v))) {
            let wit_value: WitValue = value.clone().into();
            let round_trip_value: Value = wit_value.into();
            prop_assert_eq!(value, round_trip_value);
        }
    }
}
