use std::{env, hint::black_box, time::Instant};

fn main() {
    let mut args = env::args().skip(1);
    let limit = args.next().unwrap().parse::<u32>().unwrap();
    let fallible = args.next().unwrap().parse::<bool>().unwrap();
    let rounds = args.next().unwrap().parse::<u32>().unwrap();
    let values: Vec<u32> = (0..256).collect();
    for _ in 0..rounds {
        let values = black_box(&values);
        let limit = black_box(limit);
        let start = Instant::now();
        let result: Result<Option<&u32>, &str> = if fallible {
            values
                .iter()
                .find_map(|value| {
                    if *value == 127 {
                        Some(Err("invalid"))
                    } else if *value >= limit {
                        Some(Ok(value))
                    } else {
                        None
                    }
                })
                .transpose()
        } else {
            Ok(values.iter().find(|&&value| value >= limit))
        };
        let result = black_box(result);
        eprintln!("{}", start.elapsed().as_nanos());
        match result {
            Ok(Some(value)) => println!("{value}"),
            Ok(None) => println!("missing"),
            Err(message) => println!("{message}"),
        }
    }
    println!("done");
}
