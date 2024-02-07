#[allow(unused)]
mod bindings;
mod builder;

#[cfg(feature = "protobuf")]
pub mod protobuf;


pub use bindings::golem::rpc::types::{WitNode, WitValue};
pub use builder::{NodeBuilder, WitValueExtensions};
use crate::builder::WitValueBuilder;

pub type TypeIndex = i32;

// A tree representation of Value - isomorphic to the protobuf Val type but easier to work with in Rust
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
enum Value {
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Char(char),
    String(String),
    List(Vec<Box<Value>>),
    Tuple(Vec<Box<Value>>),
    Record(Vec<Box<Value>>),
    Variant(u32, Box<Value>),
    Enum(u32),
    Flags(Vec<bool>),
    Option(Option<Box<Value>>),
    Result(Result<Box<Value>, Box<Value>>),
}

impl From<Value> for WitValue {
    fn from(value: Value) -> Self {
        let mut builder = WitValueBuilder::new();
        build_wit_value(value, &mut builder);
        builder.build()
    }
}

fn build_wit_value(value: Value, builder: &mut WitValueBuilder) -> TypeIndex {
    match value {
        Value::Bool(value) => builder.add_bool(value),
        Value::U8(value) => builder.add_u8(value),
        Value::U16(value) => builder.add_u16(value),
        Value::U32(value) => builder.add_u32(value),
        Value::U64(value) => builder.add_u64(value),
        Value::I8(value) => builder.add_s8(value),
        Value::I16(value) => builder.add_s16(value),
        Value::I32(value) => builder.add_s32(value),
        Value::I64(value) => builder.add_s64(value),
        Value::F32(value) => builder.add_f32(value),
        Value::F64(value) => builder.add_f64(value),
        Value::Char(value) => builder.add_char(value),
        Value::String(value) => builder.add_string(&value),
        Value::List(values) => {
            let list_idx = builder.add_list();
            let mut items = Vec::new();
            for value in values {
                let item_idx = build_wit_value(*value, builder);
                items.push(item_idx);
            }
            builder.finish_seq(items, list_idx);
            list_idx
        }
        Value::Tuple(values) => {
            let tuple_idx = builder.add_tuple();
            let mut items = Vec::new();
            for value in values {
                let item_idx = build_wit_value(*value, builder);
                items.push(item_idx);
            }
            builder.finish_seq(items, tuple_idx);
            tuple_idx
        }
        Value::Record(fields) => {
            let record_idx = builder.add_record();
            let mut items = Vec::new();
            for value in fields {
                let item_idx = build_wit_value(*value, builder);
                items.push(item_idx);
            }
            builder.finish_seq(items, record_idx);
            record_idx
        }
        Value::Variant(case_idx, value) => {
            let variant_idx = builder.add_variant(case_idx, -1);
            let inner_idx = build_wit_value(*value, builder);
            builder.finish_seq(vec![inner_idx], variant_idx);
            variant_idx
        },
        Value::Enum(value) => builder.add_enum_value(value),
        Value::Flags(values) => builder.add_flags(values),
        Value::Option(value) => {
            let option_idx = builder.add_option();
            if let Some(value) = value {
                let inner_idx = build_wit_value(*value, builder);
                builder.finish_seq(vec![inner_idx], option_idx);
            }
            option_idx
        },
        Value::Result(result) => {
            match result {
                Ok(ok) => {
                    let result_idx = builder.add_result_ok();
                    let inner_idx = build_wit_value(*ok, builder);
                    builder.finish_seq(vec![inner_idx], result_idx);
                    result_idx
                },
                Err(err) => {
                    let result_idx = builder.add_result_err();
                    let inner_idx = build_wit_value(*err, builder);
                    builder.finish_seq(vec![inner_idx], result_idx);
                    result_idx
                }
            }
        },
    }
}

impl From<WitValue> for Value {
    fn from(value: WitValue) -> Self {
        assert!(value.nodes.len() > 0);
        build_tree(&value.nodes[0], &value.nodes)
    }
}

fn build_tree(node: &WitNode, nodes: &[WitNode]) -> Value {
    match node {
        WitNode::RecordValue(field_indices) => {
            let mut fields = Vec::new();
            for index in field_indices {
                let value = build_tree(&nodes[*index as usize], nodes);
                fields.push( Box::new(value));
            }
            Value::Record(fields)
        }
        WitNode::VariantValue((name, inner_idx)) => {
            let value = build_tree(&nodes[*inner_idx as usize], nodes);
            Value::Variant(name.clone(), Box::new(value))
        }
        WitNode::EnumValue(value) => {
            Value::Enum(value.clone())
        }
        WitNode::FlagsValue(values) => {
            Value::Flags(values.clone())
        }
        WitNode::TupleValue(indices) => {
            let mut values = Vec::new();
            for index in indices {
                let value = build_tree(&nodes[*index as usize], nodes);
                values.push(Box::new(value));
            }
            Value::Tuple(values)
        }
        WitNode::ListValue(indices) => {
            let mut values = Vec::new();
            for index in indices {
                let value = build_tree(&nodes[*index as usize], nodes);
                values.push(Box::new(value));
            }
            Value::List(values)
        }
        WitNode::OptionValue(Some(index)) => {
            let value = build_tree(&nodes[*index as usize], nodes);
            Value::Option(Some(Box::new(value)))
        }
        WitNode::OptionValue(None) => {
            Value::Option(None)
        }
        WitNode::ResultValue(Ok(index)) => {
            let value = build_tree(&nodes[*index as usize], nodes);
            Value::Result(Ok(Box::new(value)))
        }
        WitNode::ResultValue(Err(index)) => {
            let value = build_tree(&nodes[*index as usize], nodes);
            Value::Result(Err(Box::new(value)))
        }
        WitNode::PrimU8(value) => Value::U8(*value),
        WitNode::PrimU16(value) => Value::U16(*value),
        WitNode::PrimU32(value) => Value::U32(*value),
        WitNode::PrimU64(value) => Value::U64(*value),
        WitNode::PrimS8(value) => Value::I8(*value),
        WitNode::PrimS16(value) => Value::I16(*value),
        WitNode::PrimS32(value) => Value::I32(*value),
        WitNode::PrimS64(value) => Value::I64(*value),
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

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use proptest_arbitrary_interop::{arb, arb_sized};
    use crate::{Value, WitValue};

    const CASES : u32 = 10000;
    const SIZE : usize = 4096;

    proptest! {

        #![proptest_config(ProptestConfig {
            cases: CASES, .. ProptestConfig::default()
        })]
        #[test]
        fn round_trip(value in arb_sized::<Value>(SIZE).prop_filter("Value must be equal to itself", |v| v.eq(&v))) {
            let wit_value: WitValue = value.clone().into();
            let round_trip_value: Value = wit_value.into();
            prop_assert_eq!(value, round_trip_value);
        }
    }
}