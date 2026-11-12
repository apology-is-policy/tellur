//! Typed source adapters compiled separately from the precompiled runtime.
//! Only the versioned C value protocol crosses the static-library boundary.
use super::*;

pub(crate) fn symbol(index: usize) -> String {
    format!("tellur_user_adapter_v1_{index}")
}

pub(crate) fn validate_profile(manifest: &Manifest) -> Result<()> {
    let Some(config) = &manifest.rust else {
        return Ok(());
    };
    if !config.resources.is_empty()
        || manifest
            .primitives
            .iter()
            .filter(|p| p.rust.is_some())
            .any(|p| {
                p.asynchronous
                    || p.pure
                    || !p.returns
                    || !matches!(
                        p.rust.as_ref().unwrap().execution,
                        Execution::Immediate | Execution::Blocking
                    )
                    || p.parameters.iter().any(|p| !supported(&p.ty))
                    || p.outcomes
                        .iter()
                        .flat_map(|o| &o.fields)
                        .any(|f| !supported(&f.ty))
            })
    {
        return Err(error(
            "LLVM Rust adapters currently require synchronous scalar/collection data; resources and polled operations are not yet supported",
        ));
    }
    Ok(())
}

const ABI: &str = r#"
use std::ffi::c_void;
unsafe extern "C" {
    fn tellur_adapter_v1_ok(c:*mut c_void)->u32;
    fn tellur_adapter_v1_fail(c:*mut c_void,data:*const u8,len:usize);
    fn tellur_adapter_v1_number(c:*mut c_void,h:u64,tag:u32)->u64;
    fn tellur_adapter_v1_bytes(c:*mut c_void,h:u64,tag:u32,out:*mut u8,len:usize)->usize;
    fn tellur_adapter_v1_value(c:*mut c_void,tag:u32,n:u64,data:*const u8,len:usize)->u64;
    fn tellur_adapter_v1_length(c:*mut c_void,h:u64)->usize;
    fn tellur_adapter_v1_unpack(c:*mut c_void,h:u64,out:*mut u64,count:usize);
    fn tellur_adapter_v1_collection(c:*mut c_void,ty:*const u8,len:usize,fields:*const u64,count:usize)->u64;
    fn tellur_adapter_v1_drop(c:*mut c_void,h:u64);
    fn tellur_adapter_v1_complete(c:*mut c_void,name:*const u8,len:usize,fields:*const u64,count:usize);
}
type AdapterResult<T> = Result<T,()>;
unsafe fn check<T>(c:*mut c_void,value:T)->AdapterResult<T> {
    if unsafe {tellur_adapter_v1_ok(c)} != 0 {Ok(value)} else {Err(())}
}
unsafe fn fail(c:*mut c_void,message:&str) {
    unsafe {tellur_adapter_v1_fail(c,message.as_ptr(),message.len())};
}
"#;

pub(crate) fn source(manifest: &Manifest) -> Result<String> {
    validate_profile(manifest)?;
    let config = manifest
        .rust
        .as_ref()
        .ok_or_else(|| error("missing Rust adapter configuration"))?;
    if !config.source.is_absolute()
        || !config.lockfile.is_absolute()
        || config.modules.values().any(|p| !p.is_absolute())
    {
        return Err(error(
            "native adapter paths are unresolved; load the manifest with Manifest::from_json_at",
        ));
    }
    let read = |path: &std::path::Path, label: &str| {
        std::fs::read_to_string(path).map_err(|e| {
            error(format!(
                "cannot read Rust adapter {label}{}: {e}",
                path.display()
            ))
        })
    };
    let host = read(&config.source, "")?;
    let mut code = String::from("#![allow(unused, non_camel_case_types)]\n");
    code.push_str(ABI);
    code.push_str("pub mod rust_api {\n");
    let mut all = BTreeSet::new();
    for p in manifest.primitives.iter().filter(|p| p.rust.is_some()) {
        writeln!(code, "pub enum {}_outcome {{", p.name).unwrap();
        for o in &p.outcomes {
            if o.fields.is_empty() {
                writeln!(code, "r#{},", o.name).unwrap();
            } else {
                writeln!(
                    code,
                    "r#{}({}),",
                    o.name,
                    o.fields
                        .iter()
                        .map(|f| rust_type(&f.ty, &config.resources))
                        .collect::<Vec<_>>()
                        .join(",")
                )
                .unwrap();
            }
            for f in &o.fields {
                collect_type(&f.ty, &mut all);
            }
        }
        code.push_str("}\n");
        for f in &p.parameters {
            collect_type(&f.ty, &mut all);
        }
    }
    code.push_str("}\nmod rust_host {\n");
    code.push_str(&host);
    for (name, path) in &config.modules {
        writeln!(
            code,
            "\nmod r#{name} {{\n{}\n}}",
            read(path, &format!("module `{name}` at "))?
        )
        .unwrap();
    }
    code.push_str("\n}\n");
    let ids: BTreeMap<_, _> = all
        .iter()
        .enumerate()
        .map(|(i, t)| (t.clone(), i))
        .collect();
    for t in &all {
        let id = ids[t];
        let ty = rust_type(t, &config.resources);
        let (decode, encode) = match t.as_str() {
            "Unit" | "Bool" | "U32" => {
                let (tag, value, number) = match t.as_str() {
                    "Unit" => (0, "()", "0"),
                    "Bool" => (1, "number != 0", "u64::from(value)"),
                    _ => (2, "number as u32", "u64::from(value)"),
                };
                (
                    format!(
                        "let number=tellur_adapter_v1_number(c,h,{tag});check(c,())?;tellur_adapter_v1_drop(c,h);check(c,{value})"
                    ),
                    format!(
                        "let h=tellur_adapter_v1_value(c,{tag},{number},std::ptr::null(),0);check(c,h)"
                    ),
                )
            }
            "String" | "Bytes" => {
                let tag = if t == "String" { 3 } else { 4 };
                let value = if t == "String" {
                    "String::from_utf8(data).map_err(|_|fail(c,\"invalid UTF-8 from runtime\"))?"
                } else {
                    "data"
                };
                (
                    format!(
                        "let len=tellur_adapter_v1_bytes(c,h,{tag},std::ptr::null_mut(),0);check(c,())?;let mut data=vec![0;len];tellur_adapter_v1_bytes(c,h,{tag},data.as_mut_ptr(),len);check(c,())?;tellur_adapter_v1_drop(c,h);check(c,{value})"
                    ),
                    format!(
                        "let h=tellur_adapter_v1_value(c,{tag},0,value.as_ptr(),value.len());check(c,h)"
                    ),
                )
            }
            _ => {
                let pre = "let len=tellur_adapter_v1_length(c,h);check(c,())?;let mut fields=vec![0u64;len];tellur_adapter_v1_unpack(c,h,fields.as_mut_ptr(),len);check(c,())?;";
                let post = format!(
                    "let ty={t:?};let h=tellur_adapter_v1_collection(c,ty.as_ptr(),ty.len(),fields.as_ptr(),fields.len());check(c,h)"
                );
                match Collection::parse(t).unwrap() {
                    Collection::Vector(inner) => (
                        format!(
                            "{pre}fields.into_iter().map(|h|decode_{}(c,h)).collect()",
                            ids[&inner]
                        ),
                        format!(
                            "let fields=value.into_iter().map(|v|encode_{}(c,v)).collect::<AdapterResult<Vec<_>>>()?;{post}",
                            ids[&inner]
                        ),
                    ),
                    Collection::Map(k, v) => (
                        format!(
                            "{pre}if len%2!=0 {{fail(c,\"invalid map arity\");return Err(());}}let mut fields=fields.into_iter();let mut value=std::collections::BTreeMap::new();while let Some(k)=fields.next() {{let k=decode_{}(c,k)?;let v=decode_{}(c,fields.next().unwrap())?;value.insert(k,v);}}Ok(value)",
                            ids[&k], ids[&v]
                        ),
                        format!(
                            "let mut fields=Vec::with_capacity(value.len()*2);for (k,v) in value {{fields.push(encode_{}(c,k)?);fields.push(encode_{}(c,v)?);}}{post}",
                            ids[&k], ids[&v]
                        ),
                    ),
                    _ => unreachable!(),
                }
            }
        };
        writeln!(code,"unsafe fn decode_{id}(c:*mut c_void,h:u64)->AdapterResult<{ty}>{{unsafe{{{decode}}}}}\nunsafe fn encode_{id}(c:*mut c_void,value:{ty})->AdapterResult<u64>{{unsafe{{{encode}}}}}").unwrap();
    }
    for (index, p) in manifest
        .primitives
        .iter()
        .filter(|p| p.rust.is_some())
        .enumerate()
    {
        writeln!(code,"#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn {}(c:*mut c_void,args:*const u64,count:usize){{let _=unsafe{{invoke_{index}(c,args,count)}};}}",symbol(index)).unwrap();
        writeln!(code,"unsafe fn invoke_{index}(c:*mut c_void,args:*const u64,count:usize)->AdapterResult<()>{{unsafe{{if count!={} {{fail(c,\"adapter argument arity mismatch\");return Err(());}}let args=if count==0{{&[]}}else{{std::slice::from_raw_parts(args,count)}};",p.parameters.len()).unwrap();
        for (i, f) in p.parameters.iter().enumerate() {
            writeln!(code, "let p{i}=decode_{}(c,args[{i}])?;", ids[&f.ty]).unwrap();
        }
        let types = p
            .parameters
            .iter()
            .map(|p| {
                if p.mode != Mode::Borrow {
                    return rust_type(&p.ty, &config.resources);
                }
                match p.ty.as_str() {
                    "String" => "&str".into(),
                    "Bytes" => "&[u8]".into(),
                    _ => match Collection::parse(&p.ty) {
                        Some(Collection::Vector(t)) => {
                            format!("&[{}]", rust_type(&t, &config.resources))
                        }
                        _ => format!("&{}", rust_type(&p.ty, &config.resources)),
                    },
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let args = p
            .parameters
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{}p{i}", if p.mode == Mode::Borrow { "&" } else { "" }))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(code,"check(c,())?;let call:fn({types})->rust_api::{}_outcome=rust_host::r#{};match call({args}){{",p.name,p.name).unwrap();
        for o in &p.outcomes {
            let pattern = if o.fields.is_empty() {
                String::new()
            } else {
                format!(
                    "({})",
                    (0..o.fields.len())
                        .map(|i| format!("v{i}"))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            let fields = o
                .fields
                .iter()
                .enumerate()
                .map(|(i, f)| format!("encode_{}(c,v{i})?", ids[&f.ty]))
                .collect::<Vec<_>>()
                .join(",");
            writeln!(code,"rust_api::{}_outcome::r#{}{pattern}=>{{let fields:[u64;{}]=[{fields}];let name={:?};tellur_adapter_v1_complete(c,name.as_ptr(),name.len(),fields.as_ptr(),fields.len());check(c,())}},",p.name,o.name,o.fields.len(),o.name).unwrap();
        }
        code.push_str("}}}\n");
    }
    Ok(code)
}
