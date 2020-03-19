use std::collections::HashMap;

use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;
use num_traits::sign::Signed;

use super::Builtin;
use crate::common::QuoteKind;
use crate::unit::Unit;
use crate::value::{Number, Value};

pub(crate) fn register(f: &mut HashMap<String, Builtin>) {
    f.insert(
        "to-upper-case".to_owned(),
        Box::new(|args, _| {
            max_args!(args, 1);
            match arg!(args, 0, "string") {
                Value::Ident(i, q) => Ok(Value::Ident(i.to_ascii_uppercase(), q)),
                v => Err(format!("$string: {} is not a string.", v).into()),
            }
        }),
    );
    f.insert(
        "to-lower-case".to_owned(),
        Box::new(|args, _| {
            max_args!(args, 1);
            match arg!(args, 0, "string") {
                Value::Ident(i, q) => Ok(Value::Ident(i.to_ascii_lowercase(), q)),
                v => Err(format!("$string: {} is not a string.", v).into()),
            }
        }),
    );
    f.insert(
        "str-length".to_owned(),
        Box::new(|args, _| {
            max_args!(args, 1);
            match arg!(args, 0, "string") {
                Value::Ident(i, _) => Ok(Value::Dimension(Number::from(i.len()), Unit::None)),
                v => Err(format!("$string: {} is not a string.", v).into()),
            }
        }),
    );
    f.insert(
        "quote".to_owned(),
        Box::new(|args, _| {
            max_args!(args, 1);
            match arg!(args, 0, "string") {
                Value::Ident(i, _) => Ok(Value::Ident(i, QuoteKind::Double)),
                v => Err(format!("$string: {} is not a string.", v).into()),
            }
        }),
    );
    f.insert(
        "unquote".to_owned(),
        Box::new(|args, _| {
            max_args!(args, 1);
            match arg!(args, 0, "string") {
                Value::Ident(i, _) if i.is_empty() => Ok(Value::Null),
                i @ Value::Ident(..) => Ok(i.unquote()),
                v => Err(format!("$string: {} is not a string.", v).into()),
            }
        }),
    );
    f.insert(
        "str-slice".to_owned(),
        Box::new(|args, _| {
            max_args!(args, 3);
            let (string, quotes) = match arg!(args, 0, "string") {
                Value::Ident(s, q) => (s, q),
                v => return Err(format!("$string: {} is not a string.", v).into()),
            };
            let str_len = string.len();
            let start = match arg!(args, 1, "start-at") {
                Value::Dimension(n, Unit::None) if n.is_decimal() => {
                    return Err(format!("{} is not an int.", n).into())
                }
                Value::Dimension(n, Unit::None) if n.to_integer().is_positive() => {
                    n.to_integer().to_usize().unwrap()
                }
                Value::Dimension(n, Unit::None) if n == Number::from(0) => 1_usize,
                Value::Dimension(n, Unit::None) if n < -Number::from(str_len) => 1_usize,
                Value::Dimension(n, Unit::None) => (BigInt::from(str_len + 1) + n.to_integer())
                    .to_usize()
                    .unwrap(),
                v @ Value::Dimension(..) => {
                    return Err(format!("$start: Expected {} to have no units.", v).into())
                }
                v => return Err(format!("$start-at: {} is not a number.", v).into()),
            };
            let mut end = match arg!(args, 2, "end-at" = Value::Null) {
                Value::Dimension(n, Unit::None) if n.is_decimal() => {
                    return Err(format!("{} is not an int.", n).into())
                }
                Value::Dimension(n, Unit::None) if n.to_integer().is_positive() => {
                    n.to_integer().to_usize().unwrap()
                }
                Value::Dimension(n, Unit::None) if n == Number::from(0) => 0_usize,
                Value::Dimension(n, Unit::None) if n < -Number::from(str_len) => 0_usize,
                Value::Dimension(n, Unit::None) => (BigInt::from(str_len + 1) + n.to_integer())
                    .to_usize()
                    .unwrap(),
                v @ Value::Dimension(..) => {
                    return Err(format!("$end: Expected {} to have no units.", v).into())
                }
                Value::Null => str_len,
                v => return Err(format!("$end-at: {} is not a number.", v).into()),
            };

            if end > str_len {
                end = str_len;
            }

            if start > end || start > str_len {
                match quotes {
                    QuoteKind::Double | QuoteKind::Single => {
                        Ok(Value::Ident(String::new(), QuoteKind::Double))
                    }
                    QuoteKind::None => Ok(Value::Null),
                }
            } else {
                let s = string[start - 1..end].to_string();
                match quotes {
                    QuoteKind::Double | QuoteKind::Single => Ok(Value::Ident(s, QuoteKind::Double)),
                    QuoteKind::None => Ok(Value::Ident(s, QuoteKind::None)),
                }
            }
        }),
    );
}
