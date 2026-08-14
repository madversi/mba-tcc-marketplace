use chrono::{DateTime, SubsecRound, Utc};

pub fn now() -> DateTime<Utc> {
    Utc::now().trunc_subsecs(6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_nao_tem_nanossegundos() {
        let instant = now();

        assert_eq!(instant.timestamp_subsec_nanos() % 1_000, 0);
    }
}
