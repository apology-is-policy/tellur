//! Compile the same private contract used by generated continuations.
use super::*;

fn compile(name: &str, body: &str, expected: Option<&str>) {
    let root =
        std::env::temp_dir().join(format!("tellur-owned-type-{name}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("main.rs");
    fs::write(&source,format!("#![allow(dead_code)]\nmod contract {{\n{CONTRACT}\n}}\nuse contract::{{Adapter,Operation,Poll,ReadInterest}};\n{body}")).unwrap();
    let binary = root.join("probe");
    let compiled = Command::new("rustc")
        .args(["--edition=2024", "--error-format=json"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    let stderr = String::from_utf8(compiled.stderr).unwrap();
    if let Some(expected) = expected {
        assert!(!compiled.status.success(), "{name} unexpectedly accepted");
        let errors = stderr
            .lines()
            .filter_map(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .filter(|d| d["level"] == "error")
            .collect::<Vec<_>>();
        assert!(
            errors
                .iter()
                .any(|d| d["message"].as_str().unwrap().contains(expected)),
            "{name}: {stderr}"
        );
    } else {
        assert!(compiled.status.success(), "{name}: {stderr}");
        let run = Command::new(&binary).output().unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn owned_operation_rejects_borrows_escaping_preparation_poll_or_result() {
    let template = r#"
struct Read;
impl Adapter for Read {
    type Resource=String;
    type State=STATE;
    type Outcome=OUTPUT;
    fn prepare(resource:&String,request:String,_:u64)->Self::State { PREPARE }
    fn poll(state:&mut Self::State,resource:&String,bytes:&[u8])->Poll<Self::Outcome> { BODY }
}
fn main() {}
"#;
    for (name, state, out, prepare, body) in [
        (
            "prepare-borrow",
            "&'static String",
            "String",
            "resource",
            "Poll::Complete(String::new())",
        ),
        (
            "poll-resource",
            "Option<&'static String>",
            "String",
            "None",
            "*state=Some(resource); Poll::Wait(ReadInterest{deadline:57})",
        ),
        (
            "poll-bytes",
            "Option<&'static [u8]>",
            "String",
            "None",
            "*state=Some(bytes); Poll::Wait(ReadInterest{deadline:57})",
        ),
        (
            "result-resource",
            "()",
            "&'static String",
            "()",
            "Poll::Complete(resource)",
        ),
        (
            "result-bytes",
            "()",
            "&'static [u8]",
            "()",
            "Poll::Complete(bytes)",
        ),
    ] {
        let code = template
            .replace("STATE", state)
            .replace("OUTPUT", out)
            .replace("PREPARE", prepare)
            .replace("BODY", body);
        compile(name, &code, Some("lifetime may not live long enough"));
    }
}

#[test]
fn owned_operation_requires_exact_poll_and_owned_outcome_types() {
    let template = r#"
struct Read;
impl Adapter for Read {
    type Resource=String;
    type State=();
    type Outcome=String;
    fn prepare(_:&String,_:String,_:u64) {}
    fn poll(_:&mut (),_:RESOURCE,_:&[u8])->Poll<String> { BODY }
}
fn main() {}
"#;
    compile(
        "exclusive-poll",
        &template
            .replace("RESOURCE", "&mut String")
            .replace("BODY", "Poll::Complete(String::new())"),
        Some("incompatible type for trait"),
    );
    compile(
        "wrong-outcome",
        &template
            .replace("RESOURCE", "&String")
            .replace("BODY", "Poll::Complete(42u32)"),
        Some("mismatched types"),
    );
}

#[test]
fn owned_operation_moves_inputs_and_discards_pending_state_or_completion_once() {
    compile(
        "owned-lifecycle",
        r#"
use std::{cell::Cell,rc::Rc};
struct Probe {alive:Rc<Cell<bool>>,states:Rc<Cell<usize>>,results:Rc<Cell<usize>>}
impl Drop for Probe {fn drop(&mut self){assert_eq!(self.states.get(),1);assert!(self.alive.replace(false));}}
struct State {alive:Rc<Cell<bool>>,drops:Rc<Cell<usize>>,request:String,address:usize,deadline:u64,polls:u8}
impl Drop for State {fn drop(&mut self){assert!(self.alive.get());self.drops.set(self.drops.get()+1);}}
struct Output(String,Rc<Cell<usize>>);
impl Drop for Output {fn drop(&mut self){self.1.set(self.1.get()+1);}}
struct Read;
impl Adapter for Read {
    type Resource=Probe;
    type State=State;
    type Outcome=Output;
    fn prepare(p:&Probe,request:String,deadline:u64)->State {
        let address=request.as_ptr() as usize;
        State{alive:p.alive.clone(),drops:p.states.clone(),request,address,deadline,polls:0}
    }
    fn poll(s:&mut State,p:&Probe,bytes:&[u8])->Poll<Output>{
        assert!(p.alive.get());assert_eq!(s.request.as_ptr() as usize,s.address);
        s.polls+=1;
        if s.polls==1 {return Poll::Wait(ReadInterest{deadline:s.deadline});}
        assert_eq!(bytes,b"ok");
        Poll::Complete(Output(std::mem::take(&mut s.request),p.results.clone()))
    }
}
fn main() {
    for phase in 0..4 {
        let alive=Rc::new(Cell::new(true));let states=Rc::new(Cell::new(0));let results=Rc::new(Cell::new(0));
        let p=Probe{alive:alive.clone(),states:states.clone(),results:results.clone()};
        let request="moved input".repeat(20);let address=request.as_ptr();
        let mut op=Operation::<Read>::start(&p,request,57);
        assert!(op.take().is_err());assert_eq!(states.get(),0);
        if phase>0 {assert_eq!(op.poll(&p,b"").unwrap(),Some(ReadInterest{deadline:57}));assert!(op.take().is_err());assert_eq!(states.get(),0);}
        if phase>1 {assert!(op.poll(&p,b"ok").unwrap().is_none());assert_eq!(states.get(),1);assert_eq!(results.get(),0);assert!(op.poll(&p,b"twice").is_err());}
        if phase==3 {let output=op.take().unwrap();assert_eq!(output.0.as_ptr(),address);assert!(op.take().is_err());assert!(op.poll(&p,b"late").is_err());drop(output);}
        drop(op);assert_eq!(states.get(),1);assert_eq!(results.get(),usize::from(phase>1));drop(p);assert!(!alive.get());
    }
}
"#,
        None,
    );
}
