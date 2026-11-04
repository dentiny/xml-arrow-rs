use arrow_array::builder::{
    ArrayBuilder, BooleanBuilder, Float64Builder, Int64Builder, ListBuilder, StringBuilder,
    StructBuilder,
};
use arrow_schema::DataType;

use crate::{parse::Value, XmlField};

pub(crate) fn append(builder: &mut dyn ArrayBuilder, field: &XmlField, value: Option<Value>) {
    macro_rules! scalar {
        ($builder:ty, $variant:ident) => {
            builder
                .as_any_mut()
                .downcast_mut::<$builder>()
                .unwrap()
                .append_option(value.map(|value| match value {
                    Value::$variant(value) => value,
                    _ => unreachable!("parsed values match the validated schema"),
                }))
        };
    }
    match field.field.data_type() {
        DataType::Utf8 => scalar!(StringBuilder, Text),
        DataType::Boolean => scalar!(BooleanBuilder, Boolean),
        DataType::Int64 => scalar!(Int64Builder, Integer),
        DataType::Float64 => scalar!(Float64Builder, Float),
        DataType::Struct(_) => {
            let builder = builder
                .as_any_mut()
                .downcast_mut::<StructBuilder>()
                .unwrap();
            let valid = value.is_some();
            let values = match value {
                Some(Value::Struct(values)) => values,
                None => (0..field.children.len()).map(|_| None).collect(),
                _ => unreachable!("parsed values match the validated schema"),
            };
            for ((builder, field), value) in builder
                .field_builders_mut()
                .iter_mut()
                .zip(&field.children)
                .zip(values)
            {
                append(builder.as_mut(), field, value);
            }
            builder.append(valid);
        }
        DataType::List(_) => {
            let builder = builder
                .as_any_mut()
                .downcast_mut::<ListBuilder<Box<dyn ArrayBuilder>>>()
                .unwrap();
            let valid = value.is_some();
            match value {
                Some(Value::List(values)) => {
                    for value in values {
                        append(builder.values().as_mut(), &field.children[0], value);
                    }
                }
                None => {}
                _ => unreachable!("parsed values match the validated schema"),
            }
            builder.append(valid);
        }
        _ => unreachable!("schema types are validated on construction"),
    }
}
