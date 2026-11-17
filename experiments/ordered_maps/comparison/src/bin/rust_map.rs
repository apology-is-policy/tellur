use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, env, fs, hint::black_box, time::Instant};

#[derive(Deserialize)]
struct Scenario {
    entry: String,
    arguments: [Argument; 1],
}

#[derive(Deserialize)]
struct Argument {
    value: MapFields,
}

#[derive(Deserialize)]
struct MapFields {
    fields: Vec<Field>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
enum Field {
    String(String),
    U32(u32),
}

#[derive(Serialize)]
struct Event<T = String> {
    kind: &'static str,
    message: T,
}
#[derive(Serialize)]
struct Report<T> {
    events: T,
    fields: [(); 0],
    outcome: &'static str,
}

struct PooledTrace {
    text: String,
    ends: Vec<usize>,
}
impl PooledTrace {
    fn emit_pair(&mut self, key: &str, value: u32) {
        use std::fmt::Write;
        self.text.push_str(key);
        self.ends.push(self.text.len());
        write!(self.text, "{value}").unwrap();
        self.ends.push(self.text.len());
    }
}
impl Serialize for PooledTrace {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut start = 0;
        serializer.collect_seq(self.ends.iter().map(|&end| {
            let message = &self.text[start..end];
            start = end;
            Event {
                kind: "emit",
                message,
            }
        }))
    }
}

fn visit(map: &BTreeMap<String, u32>) -> usize {
    let mut count = 0;
    for pair in map {
        black_box(pair);
        count += 1;
    }
    count
}

fn main() {
    let mut args = env::args().skip(1);
    let input = fs::read_to_string(args.next().unwrap()).unwrap();
    let scenario: Scenario = serde_json::from_str(&input).unwrap();
    let [arg] = scenario.arguments;
    let mut fields = arg.value.fields.into_iter();
    let mut map = BTreeMap::new();
    while let Some(field) = fields.next() {
        let Field::String(key) = field else {
            panic!("missing key")
        };
        let Some(Field::U32(value)) = fields.next() else {
            panic!("missing value")
        };
        assert!(map.insert(key, value).is_none());
    }
    let mut events = Vec::new();
    match scenario.entry.as_str() {
        "accept" => {
            black_box(&map);
        }
        "scan" => {
            if let Some(rounds) = args.next() {
                let rounds: usize = rounds.parse().unwrap();
                let start = Instant::now();
                for _ in 0..rounds {
                    assert_eq!(visit(black_box(&map)), map.len());
                }
                println!("{}", start.elapsed().as_nanos());
                return;
            }
            assert_eq!(visit(&map), map.len());
        }
        "observe_scan" | "observe_drain" => {
            let drain = scenario.entry == "observe_drain";
            let layout = args.next();
            if matches!(layout.as_deref(), Some("--pooled" | "--into-iter")) {
                let timer = env::var_os("VN_MAP_TIMER").map(|_| Instant::now());
                let mut trace = PooledTrace {
                    text: String::new(),
                    ends: Vec::with_capacity(2 * map.len()),
                };
                if drain && layout.as_deref() == Some("--into-iter") {
                    for (key, value) in map {
                        trace.emit_pair(&key, value);
                    }
                } else if drain {
                    while let Some((key, value)) = map.pop_first() {
                        trace.emit_pair(&key, value);
                    }
                } else {
                    for (key, value) in &map {
                        trace.emit_pair(key, *value);
                    }
                }
                if let Some(start) = timer {
                    eprintln!("{}", start.elapsed().as_nanos());
                }
                println!(
                    "{}",
                    serde_json::to_string(&Report {
                        events: trace,
                        fields: [],
                        outcome: "done"
                    })
                    .unwrap()
                );
                return;
            }
            let timer = env::var_os("VN_MAP_TIMER").map(|_| Instant::now());
            let mut emit = |key: String, value: u32| {
                events.push(Event {
                    kind: "emit",
                    message: key,
                });
                events.push(Event {
                    kind: "emit",
                    message: value.to_string(),
                });
            };
            if drain {
                while let Some((key, value)) = map.pop_first() {
                    emit(key, value);
                }
            } else {
                for (key, value) in &map {
                    emit(key.clone(), *value);
                }
            }
            if let Some(start) = timer {
                eprintln!("{}", start.elapsed().as_nanos());
            }
        }
        "drain" => {
            while let Some(entry) = map.pop_first() {
                black_box(entry);
            }
        }
        _ => panic!("unknown benchmark entry"),
    }
    println!(
        "{}",
        serde_json::to_string(&Report {
            events,
            fields: [],
            outcome: "done"
        })
        .unwrap()
    );
}
