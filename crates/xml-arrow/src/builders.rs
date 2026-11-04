use arrow_array::builder::{
    ArrayBuilder, BooleanBuilder, Float64Builder, Int64Builder, ListBuilder, StringBuilder,
    StructBuilder,
};
use arrow_schema::DataType;

use crate::{parse::Value, XmlField};

pub(crate) fn append(builder: &mut dyn ArrayBuilder, field: &XmlField, value: Value) {
    let valid = !matches!(value, Value::Null);
    macro_rules! scalar {
        ($builder:ty, $variant:ident) => {
            builder
                .as_any_mut()
                .downcast_mut::<$builder>()
                .unwrap()
                .append_option(match value {
                    Value::Null => None,
                    Value::$variant(value) => Some(value),
                    _ => unreachable!("parsed values match the validated schema"),
                })
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
            let mut values = match value {
                Value::Struct(values) => values.into_iter(),
                Value::Null => Vec::new().into_iter(),
                _ => unreachable!("parsed values match the validated schema"),
            };
            for (builder, field) in builder.field_builders_mut().iter_mut().zip(&field.children) {
                append(
                    builder.as_mut(),
                    field,
                    values.next().unwrap_or(Value::Null),
                );
            }
            builder.append(valid);
        }
        DataType::List(_) => {
            let builder = builder
                .as_any_mut()
                .downcast_mut::<ListBuilder<Box<dyn ArrayBuilder>>>()
                .unwrap();
            match value {
                Value::List(values) => {
                    for value in values {
                        append(builder.values().as_mut(), &field.children[0], value);
                    }
                }
                Value::Null => {}
                _ => unreachable!("parsed values match the validated schema"),
            }
            builder.append(valid);
        }
        _ => unreachable!("schema types are validated on construction"),
    }
}
