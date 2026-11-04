use arrow_array::builder::{
    ArrayBuilder, BooleanBuilder, Float64Builder, Int64Builder, ListBuilder, StringBuilder,
    StructBuilder,
};
use arrow_schema::DataType;

use crate::{parse::Value, Result, XmlField};

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

// All items in a row share each nested Arrow builder. Check their cumulative
// offsets together before append, including for lists of lists or structs.
pub(crate) fn validate_capacity(
    builder: &dyn ArrayBuilder,
    field: &XmlField,
    values: &[&Value],
) -> Result<()> {
    match field.field.data_type() {
        DataType::Utf8 => {
            let builder = builder.as_any().downcast_ref::<StringBuilder>().unwrap();
            let bytes = values
                .iter()
                .filter_map(|value| match value {
                    Value::Text(text) => Some(text.len()),
                    _ => None,
                })
                .fold(0usize, usize::saturating_add);
            if builder.values_slice().len().saturating_add(bytes) > i32::MAX as usize {
                return Err(
                    field.error("Utf8 offset limit exceeded; use a smaller batch or record")
                );
            }
        }
        DataType::Struct(_) => {
            let builder = builder.as_any().downcast_ref::<StructBuilder>().unwrap();
            for (index, (builder, child)) in builder
                .field_builders()
                .iter()
                .zip(&field.children)
                .enumerate()
            {
                let children: Vec<_> = values
                    .iter()
                    .filter_map(|value| match value {
                        Value::Struct(children) => Some(&children[index]),
                        _ => None,
                    })
                    .collect();
                validate_capacity(builder.as_ref(), child, &children)?;
            }
        }
        DataType::List(_) => {
            let builder = builder
                .as_any()
                .downcast_ref::<ListBuilder<Box<dyn ArrayBuilder>>>()
                .unwrap();
            let children: Vec<_> = values
                .iter()
                .filter_map(|value| match value {
                    Value::List(children) => Some(children),
                    _ => None,
                })
                .flatten()
                .collect();
            if builder.values_ref().len().saturating_add(children.len()) > i32::MAX as usize {
                return Err(
                    field.error("List offset limit exceeded; use a smaller batch or record")
                );
            }
            validate_capacity(builder.values_ref().as_ref(), &field.children[0], &children)?;
        }
        _ => {}
    }
    Ok(())
}
