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
        "observe_scan" => {
            if args.next().as_deref() == Some("--pooled") {
                use std::fmt::Write;
                let timer = env::var_os("VN_MAP_TIMER").map(|_| Instant::now());
                let mut trace = PooledTrace {
                    text: String::new(),
                    ends: Vec::with_capacity(2 * map.len()),
                };
                for (key, value) in &map {
                    trace.text.push_str(key);
                    trace.ends.push(trace.text.len());
                    write!(trace.text, "{value}").unwrap();
                    trace.ends.push(trace.text.len());
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
            for (key, val) in &map {
                events.push(Event {
                    kind: "emit",
                    message: key.clone(),
                });
                events.push(Event {
                    kind: "emit",
                    message: val.to_string(),
                });
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
