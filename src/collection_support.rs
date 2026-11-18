// Shared by the reference interpreter and generated native programs.
use std::cmp::Ordering;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Collection {
    Array(String, usize),
    Vector(String),
    Map(String, String),
}
/// Split only top-level generic arguments; malformed spellings are rejected.
pub fn generic(ty: &str) -> Option<(&str, Vec<&str>)> {
    let (name, rest) = ty.split_once('<')?;
    let rest = rest.strip_suffix('>')?;
    let (mut depth, mut start) = (0usize, 0usize);
    let mut args = vec![];
    for (i, c) in rest.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                args.push(rest[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    args.push(rest[start..].trim());
    if args.iter().any(|s| s.is_empty()) {
        return None;
    }
    Some((name, args))
}
/// A member of a balanced generic application, such as reader<vector<u32>>.item.
pub fn qualified_member(name: &str) -> Option<(&str, &str)> {
    let start = name.find('<')?;
    let mut depth = 0usize;
    for (i, c) in name.char_indices().skip_while(|(i, _)| *i < start) {
        match c {
            '<' => depth += 1,
            '>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return name[i + 1..]
                        .strip_prefix('.')
                        .filter(|s| !s.is_empty())
                        .map(|member| (&name[..=i], member));
                }
            }
            _ => (),
        }
    }
    None
}
impl Collection {
    pub fn parse(ty: &str) -> Option<Self> {
        let (name, args) = generic(ty)?;
        match (name, args.as_slice()) {
            ("vector", [t]) => Some(Self::Vector((*t).into())),
            ("array", [t, n]) if n.bytes().all(|c| c.is_ascii_digit()) => {
                let n = n.parse::<u32>().ok()? as usize;
                Some(Self::Array((*t).into(), n))
            }
            ("map", [k, v]) => Some(Self::Map((*k).into(), (*v).into())),
            _ => None,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Array(..) => "array",
            Self::Vector(_) => "vector",
            Self::Map(..) => "map",
        }
    }
    pub fn element(&self) -> &str {
        match self {
            Self::Array(t, _) | Self::Vector(t) | Self::Map(_, t) => t,
        }
    }
    pub fn shared_reference_element(&self) -> bool {
        self.element().starts_with('&') && !self.element().starts_with("&mutable ")
    }
    pub fn types(&self) -> Vec<&str> {
        match self {
            Self::Map(k, v) => vec![k, v],
            _ => vec![self.element()],
        }
    }
    pub fn field_types(&self, len: usize) -> Option<impl Iterator<Item = &str>> {
        if len > u32::MAX as usize {
            return None;
        }
        let (even, odd) = match self {
            Self::Array(t, n) if *n == len => (t, t),
            Self::Vector(t) => (t, t),
            Self::Map(k, v) if len.is_multiple_of(2) => (k, v),
            _ => return None,
        };
        Some((0..len).map(move |i| {
            if i % 2 == 0 {
                even.as_str()
            } else {
                odd.as_str()
            }
        }))
    }
    pub fn fields(&self, len: usize) -> Option<Vec<String>> {
        Some(self.field_types(len)?.map(str::to_owned).collect())
    }
}
pub trait CollectionValue: Sized + Clone {
    fn parts(&self) -> Option<(&str, &str, &[Self])>;
    fn parts_mut(&mut self) -> Option<&mut Vec<Self>>;
    fn construct(ty: String, name: String, fields: Vec<Self>) -> Self;
    fn from_number(value: u32) -> Self;
    fn number(&self) -> Option<u32>;
    fn key_cmp(&self, other: &Self) -> Option<Ordering>;
}
pub enum Payload<V> {
    Owned(V),
    Borrow(usize, String),
}
pub fn sorted_map<V: CollectionValue>(fields: &[V]) -> bool {
    fields.len().is_multiple_of(2)
        && fields
            .windows(3)
            .step_by(2)
            .all(|w| w[0].key_cmp(&w[2]) == Some(Ordering::Less))
}

/// Length needs the collection shape, not owned copies of its type arguments.
/// Keep the same balanced, trimmed generic grammar as `Collection::parse`.
pub(crate) fn length_is_map(ty: &str) -> Option<bool> {
    let (name, rest) = ty.split_once('<')?;
    let rest = rest.strip_suffix('>')?;
    let arity = match name {
        "vector" => 1,
        "array" | "map" => 2,
        _ => return None,
    };
    let mut arguments = [""; 2];
    let (mut depth, mut start, mut argument) = (0usize, 0, 0);
    for (index, character) in rest.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                if argument + 1 >= arity {
                    return None;
                }
                arguments[argument] = rest[start..index].trim();
                argument += 1;
                start = index + 1;
            }
            _ => (),
        }
    }
    if depth != 0 || argument + 1 != arity {
        return None;
    }
    arguments[argument] = rest[start..].trim();
    if arguments[..arity]
        .iter()
        .any(|argument| argument.is_empty())
    {
        return None;
    }
    if name == "array" {
        let length = arguments[1];
        if !length.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        length.parse::<u32>().ok()?;
    }
    Some(name == "map")
}

#[cfg(test)]
#[test]
fn length_shape_matches_the_owned_type_parser() {
    let check = |ty: &str| {
        assert_eq!(
            length_is_map(ty),
            Collection::parse(ty).map(|c| matches!(c, Collection::Map(..))),
            "{ty:?}"
        );
    };
    let arguments = [
        "",
        "u32",
        " string ",
        "vector<u32>",
        "map<string,vector<u32>>",
        "array<array<u32,2>,3>",
        "&mutable vector<u32>",
        "0",
        "0002",
        "4294967295",
        "4294967296",
        "+2",
        "-1",
        "2.0",
        "é",
        ",",
        "<",
        ">",
        "x>y<z",
        " \t ",
    ];
    for name in ["vector", "array", "map", "other", " vector"] {
        check(name);
        for first in arguments {
            check(&format!("{name}<{first}>"));
            for second in arguments {
                check(&format!("{name}<{first},{second}>"));
                check(&format!("{name}<{first},{second},u32>"));
            }
        }
    }
    // Differential malformed-input coverage, including UTF-8 and nested delimiters.
    let alphabet = ['<', '>', ',', ' ', '\t', 'u', '0', '1', 'é', '😀'];
    let mut state = 295u64;
    for length in 0..48 {
        for _ in 0..256 {
            let mut inner = String::new();
            for _ in 0..length {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                inner.push(alphabet[(state >> 32) as usize % alphabet.len()]);
            }
            for name in ["vector", "array", "map"] {
                check(&format!("{name}<{inner}>"));
            }
        }
    }
}

pub fn length<V: CollectionValue>(value: &V) -> Result<u32, String> {
    let (ty, _, fields) = value.parts().ok_or("invalid collection")?;
    let is_map = length_is_map(ty).ok_or("invalid collection type")?;
    let len = if is_map {
        fields.len() / 2
    } else {
        fields.len()
    };
    u32::try_from(len).map_err(|_| "collection length exceeds u32".into())
}
/// The wrapper resolves the first argument's owner address. Borrow payloads are
/// converted to addresses only there; all indices here are real storage indices.
pub fn operate<V: CollectionValue>(
    name: &str,
    container: &mut V,
    args: impl Iterator<Item = V>,
) -> Result<(String, Vec<Payload<V>>), String> {
    use Payload::{Borrow, Owned};
    let (ty, _, _) = container.parts().ok_or("invalid collection value")?;
    let c = Collection::parse(ty).ok_or("invalid collection type")?;
    let op = name
        .strip_prefix(c.name())
        .and_then(|suffix| suffix.strip_prefix('_'))
        .ok_or("wrong collection operation")?;
    let fields = container.parts_mut().ok_or("invalid collection storage")?;
    let mut args = args;
    let mut next = || {
        args.next()
            .ok_or_else(|| "missing collection argument".to_owned())
    };
    let result = match op {
        "entry_at" => {
            let Collection::Map(key, value) = &c else {
                return Err("expected map".into());
            };
            let position = next()?.number().ok_or("expected map position")?;
            if (position as usize) < fields.len() / 2 {
                let i = position as usize * 2;
                let element = if c.shared_reference_element() {
                    Owned(fields[i + 1].clone())
                } else {
                    Borrow(i + 1, format!("&{value}"))
                };
                (
                    "item",
                    vec![
                        Borrow(i, format!("&{key}")),
                        element,
                        Owned(V::from_number(position + 1)),
                    ],
                )
            } else {
                ("finished", vec![])
            }
        }
        "get" | "get_mut" => {
            let index = if matches!(c, Collection::Map(..)) {
                let key = next()?;
                fields
                    .chunks_exact(2)
                    .position(|p| p[0].key_cmp(&key) == Some(Ordering::Equal))
                    .map(|i| 2 * i + 1)
            } else {
                Some(next()?.number().ok_or("expected index")? as usize)
                    .filter(|i| *i < fields.len())
            };
            match index {
                Some(i) if c.shared_reference_element() => {
                    ("found", vec![Owned(fields[i].clone())])
                }
                Some(i) => (
                    "found",
                    vec![Borrow(
                        i,
                        format!(
                            "&{}{}",
                            if op == "get_mut" { "mutable " } else { "" },
                            c.element()
                        ),
                    )],
                ),
                None => (
                    if matches!(c, Collection::Map(..)) {
                        "missing"
                    } else {
                        "out_of_bounds"
                    },
                    vec![],
                ),
            }
        }
        "push" => {
            let value = next()?;
            if fields.len() == u32::MAX as usize {
                return Err("collection length exceeds u32".into());
            }
            fields.push(value);
            ("pushed", vec![])
        }
        "pop" => match fields.pop() {
            Some(v) => ("item", vec![Owned(v)]),
            None => ("empty", vec![]),
        },
        "replace" => {
            let i = next()?.number().ok_or("expected index")? as usize;
            let value = next()?;
            if let Some(old) = fields.get_mut(i) {
                ("replaced", vec![Owned(std::mem::replace(old, value))])
            } else {
                ("out_of_bounds", vec![Owned(value)])
            }
        }
        "remove" => {
            if matches!(c, Collection::Map(..)) {
                let key = next()?;
                if let Some(i) = fields
                    .chunks_exact(2)
                    .position(|p| p[0].key_cmp(&key) == Some(Ordering::Equal))
                {
                    let v = fields.remove(2 * i + 1);
                    fields.remove(2 * i);
                    ("removed", vec![Owned(v)])
                } else {
                    ("missing", vec![])
                }
            } else {
                let i = next()?.number().ok_or("expected index")? as usize;
                if i < fields.len() {
                    ("item", vec![Owned(fields.remove(i))])
                } else {
                    ("out_of_bounds", vec![])
                }
            }
        }
        "insert" => {
            let key = next()?;
            let value = next()?;
            let i = fields
                .chunks_exact(2)
                .position(|p| p[0].key_cmp(&key) != Some(Ordering::Less))
                .unwrap_or(fields.len() / 2)
                * 2;
            if i < fields.len() && fields[i].key_cmp(&key) == Some(Ordering::Equal) {
                (
                    "replaced",
                    vec![Owned(std::mem::replace(&mut fields[i + 1], value))],
                )
            } else {
                if fields.len() / 2 == u32::MAX as usize {
                    return Err("collection length exceeds u32".into());
                }
                fields.insert(i, key);
                fields.insert(i + 1, value);
                ("inserted", vec![])
            }
        }
        "take" => {
            if fields.is_empty() {
                ("empty", vec![])
            } else {
                let width = if matches!(c, Collection::Map(..)) {
                    2
                } else {
                    1
                };
                let mut payload = Vec::with_capacity(width + 1);
                payload.extend(fields.drain(..width).map(Owned));
                let remainder = std::mem::take(fields);
                let (ty, _, _) = container.parts().ok_or("invalid collection value")?;
                payload.push(Owned(V::construct(ty.into(), c.name().into(), remainder)));
                ("item", payload)
            }
        }
        "into_vector" => (
            "converted",
            vec![Owned(V::construct(
                format!("vector<{}>", c.element()),
                "vector".into(),
                std::mem::take(fields),
            ))],
        ),
        _ => return Err("unknown collection operation".into()),
    };
    if args.next().is_some() {
        return Err("too many collection arguments".into());
    }
    Ok((result.0.into(), result.1))
}
