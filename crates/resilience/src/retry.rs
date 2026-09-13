use std::future::Future;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl RetryPolicy {
    pub fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay,
        }
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let factor = 1u32
            .checked_shl(attempt.saturating_sub(1))
            .unwrap_or(u32::MAX);
        self.base_delay.saturating_mul(factor).min(self.max_delay)
    }

    pub async fn run<T, E, F, Fut>(&self, operation: F) -> Result<T, E>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let mut attempt = 1;
        loop {
            match operation().await {
                Ok(value) => return Ok(value),
                Err(err) if attempt >= self.max_attempts => return Err(err),
                Err(_) => {
                    tokio::time::sleep(self.delay_for_attempt(attempt)).await;
                    attempt += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn policy() -> RetryPolicy {
        RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(10))
    }

    #[test]
    fn primeira_tentativa_usa_o_delay_base() {
        assert_eq!(policy().delay_for_attempt(1), Duration::from_millis(1));
    }

    #[test]
    fn delay_dobra_a_cada_tentativa() {
        let p = policy();
        assert_eq!(p.delay_for_attempt(2), Duration::from_millis(2));
        assert_eq!(p.delay_for_attempt(3), Duration::from_millis(4));
    }

    #[test]
    fn delay_e_limitado_pelo_maximo() {
        assert_eq!(policy().delay_for_attempt(10), Duration::from_millis(10));
    }

    #[tokio::test]
    async fn retorna_ok_na_primeira_tentativa_sem_tentar_de_novo() {
        let attempts = Cell::new(0);
        let result = policy()
            .run(|| {
                attempts.set(attempts.get() + 1);
                async { Ok::<_, &str>(42) }
            })
            .await;

        assert_eq!(result, Ok(42));
        assert_eq!(attempts.get(), 1);
    }

    #[tokio::test]
    async fn tenta_novamente_apos_falha_e_eventualmente_sucede() {
        let attempts = Cell::new(0);
        let result = policy()
            .run(|| {
                attempts.set(attempts.get() + 1);
                async {
                    if attempts.get() < 3 {
                        Err("falhou")
                    } else {
                        Ok(42)
                    }
                }
            })
            .await;

        assert_eq!(result, Ok(42));
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn desiste_apos_max_attempts_e_retorna_o_ultimo_erro() {
        let attempts = Cell::new(0);
        let result = policy()
            .run(|| {
                attempts.set(attempts.get() + 1);
                async { Err::<i32, _>("sempre falha") }
            })
            .await;

        assert_eq!(result, Err("sempre falha"));
        assert_eq!(attempts.get(), 3);
    }
}
