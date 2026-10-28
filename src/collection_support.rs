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
    pub fn fields(&self, len: usize) -> Option<Vec<String>> {
        if len > u32::MAX as usize {
            return None;
        }
        match self {
            Self::Array(t, n) if *n == len => Some(vec![t.clone(); len]),
            Self::Vector(t) => Some(vec![t.clone(); len]),
            Self::Map(k, v) if len.is_multiple_of(2) => Some(
                (0..len)
                    .map(|i| if i % 2 == 0 { k.clone() } else { v.clone() })
                    .collect(),
            ),
            _ => None,
        }
    }
}
pub trait CollectionValue: Sized + Clone {
    fn parts(&self) -> Option<(&str, &str, &[Self])>;
    fn parts_mut(&mut self) -> Option<&mut Vec<Self>>;
    fn construct(ty: String, name: String, fields: Vec<Self>) -> Self;
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
            .chunks_exact(2)
            .map(|p| &p[0])
            .collect::<Vec<_>>()
            .windows(2)
            .all(|w| w[0].key_cmp(w[1]) == Some(Ordering::Less))
}
pub fn length<V: CollectionValue>(value: &V) -> Result<u32, String> {
    let (ty, _, fields) = value.parts().ok_or("invalid collection")?;
    let c = Collection::parse(ty).ok_or("invalid collection type")?;
    let len = if matches!(c, Collection::Map(..)) {
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
    args: Vec<V>,
) -> Result<(String, Vec<Payload<V>>), String> {
    use Payload::{Borrow, Owned};
    let (ty, _, _) = container.parts().ok_or("invalid collection value")?;
    let ty = ty.to_owned();
    let c = Collection::parse(&ty).ok_or("invalid collection type")?;
    let op = name
        .strip_prefix(&format!("{}_", c.name()))
        .ok_or("wrong collection operation")?;
    let fields = container.parts_mut().ok_or("invalid collection storage")?;
    let mut args = args.into_iter();
    let mut next = || {
        args.next()
            .ok_or_else(|| "missing collection argument".to_owned())
    };
    let result = match op {
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
                let mut payload = vec![Owned(fields.remove(0))];
                if matches!(c, Collection::Map(..)) {
                    payload.push(Owned(fields.remove(0)));
                }
                payload.push(Owned(V::construct(
                    ty,
                    c.name().into(),
                    std::mem::take(fields),
                )));
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
