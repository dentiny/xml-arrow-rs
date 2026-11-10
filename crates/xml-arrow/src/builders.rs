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
            typed_mut::<$builder>(builder).append_option(match value {
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
            let builder = typed_mut::<StructBuilder>(builder);
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
            let builder = typed_mut::<ListBuilder<Box<dyn ArrayBuilder>>>(builder);
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

// Nested items share builders, so check their combined offsets before append.
pub(crate) fn validate_capacity(
    builder: &dyn ArrayBuilder,
    field: &XmlField,
    values: &[&Value],
) -> Result<()> {
    match field.field.data_type() {
        DataType::Utf8 => {
            let builder = typed::<StringBuilder>(builder);
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
            let builder = typed::<StructBuilder>(builder);
            for (index, (builder, child)) in builder
                .field_builders()
                .iter()
                .zip(&field.children)
                .enumerate()
            {
                // A single struct is the common row-validation path. Keep its
                // child reference on the stack; lists still aggregate offsets
                // across all their items before any builder is mutated.
                if let [value] = values {
                    if let Value::Struct(children) = value {
                        validate_capacity(builder.as_ref(), child, &[&children[index]])?;
                    }
                } else {
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
        }
        DataType::List(_) => {
            let builder = typed::<ListBuilder<Box<dyn ArrayBuilder>>>(builder);
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

// Builder types come from the same validated schema used to parse values.
fn typed<T: ArrayBuilder>(builder: &dyn ArrayBuilder) -> &T {
    builder
        .as_any()
        .downcast_ref()
        .expect("builder type must match the XML schema")
}

fn typed_mut<T: ArrayBuilder>(builder: &mut dyn ArrayBuilder) -> &mut T {
    builder
        .as_any_mut()
        .downcast_mut()
        .expect("builder type must match the XML schema")
}
