// Explicit single-chain test driver for shared native execution.
pub fn drive(host: &mut Host, args: Vec<V>, control: &'static Control) -> R<Completion> {
    let quotas: Vec<usize> = std::env::var("TELLUR_NATIVE_MODEL_QUOTAS")
        .unwrap_or("1".into())
        .split(',')
        .map(|s| {
            s.parse()
                .map_err(|_| "E_NATIVE: invalid model quota".to_string())
        })
        .collect::<R<_>>()?;
    if !quotas.iter().any(|q| *q > 0) {
        return Err("E_NATIVE: model quotas cannot make progress".into());
    }
    let mut values = Frames::default();
    let mut domains = Domains::default();
    let mut machine = Machine::start(host, &mut values, &mut domains, control, args)?;
    for quota in quotas.into_iter().cycle() {
        match machine.advance(host, &mut values, &mut domains, quota) {
            Ok(Poll::Paused) => {}
            Ok(Poll::Settling(_)) => {
                return Err("E_NATIVE: independent driver has retained sibling work".into());
            }
            Ok(Poll::Complete(result)) => {
                assert!(values.is_empty() && machine.stack.is_empty());
                assert_eq!(domains.len(), 0);
                assert_eq!(domains.member_count(), 0);
                assert!(machine.advance(host, &mut values, &mut domains, 1).is_err());
                return Ok(result);
            }
            Err(error) => {
                assert!(values.is_empty() && machine.stack.is_empty());
                assert_eq!(domains.len(), 0);
                assert_eq!(domains.member_count(), 0);
                assert!(machine.advance(host, &mut values, &mut domains, 1).is_err());
                return Err(error);
            }
        }
    }
    unreachable!()
}
