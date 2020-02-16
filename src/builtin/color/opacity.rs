use std::collections::BTreeMap;

use super::Builtin;
use crate::common::QuoteKind;
use crate::value::Number;
use crate::units::Unit;
use crate::value::Value;

pub(crate) fn register(f: &mut BTreeMap<String, Builtin>) {
    decl!(f "alpha", |args, _| {
        max_args!(args, 1);
        match arg!(args, 0, "color") {
            Value::Color(c) => Ok(Value::Dimension(c.alpha(), Unit::None)),
            v => return Err(format!("$color: {} is not a color.", v).into()),
        }
    });
    decl!(f "opacity", |args, _| {
        max_args!(args, 1);
        match arg!(args, 0, "color") {
            Value::Color(c) => Ok(Value::Dimension(c.alpha(), Unit::None)),
            Value::Dimension(num, unit) => Ok(Value::Ident(format!("opacity({}{})", num , unit), QuoteKind::None)),
            v => return Err(format!("$color: {} is not a color.", v).into()),
        }
    });
    decl!(f "opacify", |args, _| {
        max_args!(args, 2);
        let color = match arg!(args, 0, "color").eval() {
            Value::Color(c) => c,
            v => return Err(format!("$color: {} is not a color.", v).into()),
        };
        let amount = match arg!(args, 1, "amount").eval() {
            Value::Dimension(n, u) => bound!("amount", n, u, 0, 1),
            v => return Err(format!("$amount: {} is not a number.", v).into()),
        };
        Ok(Value::Color(color.fade_in(amount)))
    });
    decl!(f "fade-in", |args, _| {
        max_args!(args, 2);
        let color = match arg!(args, 0, "color").eval() {
            Value::Color(c) => c,
            v => return Err(format!("$color: {} is not a color.", v).into()),
        };
        let amount = match arg!(args, 1, "amount").eval() {
            Value::Dimension(n, u) => bound!("amount", n, u, 0, 1),
            v => return Err(format!("$amount: {} is not a number.", v).into()),
        };
        Ok(Value::Color(color.fade_in(amount)))
    });
    decl!(f "transparentize", |args, _| {
        max_args!(args, 2);
        let color = match arg!(args, 0, "color").eval() {
            Value::Color(c) => c,
            v => return Err(format!("$color: {} is not a color.", v).into()),
        };
        let amount = match arg!(args, 1, "amount").eval() {
            Value::Dimension(n, u) => bound!("amount", n, u, 0, 1),
            v => return Err(format!("$amount: {} is not a number.", v).into()),
        };
        Ok(Value::Color(color.fade_out(amount)))
    });
    decl!(f "fade-out", |args, _| {
        max_args!(args, 2);
        let color = match arg!(args, 0, "color").eval() {
            Value::Color(c) => c,
            v => return Err(format!("$color: {} is not a color.", v).into()),
        };
        let amount = match arg!(args, 1, "amount").eval() {
            Value::Dimension(n, u) => bound!("amount", n, u, 0, 1),
            v => return Err(format!("$amount: {} is not a number.", v).into()),
        };
        Ok(Value::Color(color.fade_out(amount)))
    });
}