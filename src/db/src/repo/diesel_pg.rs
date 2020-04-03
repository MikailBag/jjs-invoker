use super::{InvocationsRepo, KvRepo, Repo, RunsRepo, UsersRepo};
use crate::schema::*;
use anyhow::{Context, Result};
use diesel::{dsl::*, prelude::*, r2d2::ConnectionManager};
use r2d2::{Pool, PooledConnection};

pub struct DieselRepo {
    pool: Pool<ConnectionManager<PgConnection>>,
}

impl std::fmt::Debug for DieselRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("DieselRepo").finish()
    }
}

impl DieselRepo {
    fn conn(&self) -> Result<PooledConnection<ConnectionManager<PgConnection>>> {
        self.pool.get().context("db connection failed")
    }

    pub(crate) fn new(conn_url: &str) -> Result<DieselRepo> {
        let conn_manager = ConnectionManager::new(conn_url);
        let mut pool_builder = Pool::builder();
        // TODO refactor
        if let Some(timeout) = std::env::var("JJS_DB_TIMEOUT")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
        {
            let dur = std::time::Duration::from_secs(timeout);
            pool_builder = pool_builder.connection_timeout(dur);
        }
        let pool = pool_builder.build(conn_manager)?;
        Ok(DieselRepo { pool })
    }
}

mod impl_users {
    use super::*;
    use crate::schema::users::dsl::*;

    #[async_trait::async_trait]
    impl UsersRepo for DieselRepo {
        async fn user_new(&self, user_data: NewUser) -> Result<User> {
            let user = User {
                id: uuid::Uuid::new_v4(),
                username: user_data.username,
                password_hash: user_data.password_hash,
                groups: user_data.groups,
            };
            diesel::insert_into(users)
                .values(&user)
                .execute(&self.conn()?)?;

            Ok(user)
        }

        async fn user_try_load_by_login(&self, login: &str) -> Result<Option<User>> {
            Ok(users
                .filter(username.eq(login))
                .load(&self.conn()?)?
                .into_iter()
                .next())
        }
    }
}

mod impl_invs {
    use super::*;
    use crate::schema::invocations::dsl::*;

    #[async_trait::async_trait]
    impl InvocationsRepo for DieselRepo {
        async fn inv_new(&self, inv_data: NewInvocation) -> Result<Invocation> {
            diesel::insert_into(invocations)
                .values(&inv_data)
                .get_result(&self.conn()?)
                .context("failed to create invocation")
                .map_err(Into::into)
        }

        async fn inv_last(&self, r_id: RunId) -> Result<Invocation> {
            tokio::task::block_in_place(move || {
                let query = diesel::sql_query(include_str!("get_last_run_invocation.sql"))
                    .bind::<diesel::sql_types::Integer, _>(r_id);
                let vals: Vec<Invocation> = query
                    .load(&self.conn()?)
                    .context("failed to load invocations")?;
                vals.into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Run has not invocations"))
            })
        }

        async fn inv_find_waiting(
            &self,
            offset: u32,
            count: u32,
            predicate: &mut (dyn FnMut(Invocation) -> Result<bool> + Send + Sync),
        ) -> Result<Vec<Invocation>> {
            tokio::task::block_in_place(move || {
                let conn = self.conn()?;
                conn.transaction::<_, anyhow::Error, _>(|| {
                    let query = "
    SELECT * FROM invocations
    WHERE state = 1
    ORDER BY id
    OFFSET $1 LIMIT $2
    FOR UPDATE
                ";
                    let invs: Vec<Invocation> = diesel::sql_query(query)
                        .bind::<diesel::sql_types::Integer, _>(offset as i32)
                        .bind::<diesel::sql_types::Integer, _>(count as i32)
                        .load(&self.conn()?)
                        .context("unable to load waiting invocations")?;
                    let mut filtered = Vec::new();
                    let mut to_del = Vec::new();
                    for inv in invs {
                        let inv_id = inv.id;
                        if predicate(inv.clone())? {
                            filtered.push(inv);
                            to_del.push(inv_id);
                        }
                    }
                    const STATE_DONE: i16 = InvocationState::InWork.as_int();
                    diesel::update(invocations)
                        .set(state.eq(STATE_DONE))
                        .filter(id.eq(any(&to_del)))
                        .execute(&self.conn()?)?;
                    Ok(filtered)
                })
            })
        }

        async fn inv_update(&self, inv_id: InvocationId, patch: InvocationPatch) -> Result<()> {
            tokio::task::block_in_place(|| {
                diesel::update(invocations)
                    .filter(id.eq(inv_id))
                    .set(&patch)
                    .execute(&self.conn()?)
                    .map_err(Into::into)
                    .map(drop)
            })
        }

        async fn inv_add_outcome_header(
            &self,
            inv_id: InvocationId,
            header: invoker_api::InvokeOutcomeHeader,
        ) -> Result<()> {
            let query = "
            UPDATE invocations SET
            outcome = outcome || $1
            WHERE id = $2
            ";
            diesel::sql_query(query)
                .bind::<diesel::sql_types::Jsonb, _>(
                    serde_json::to_value(&header)
                        .context("failed to serialize InvokeOutcomeHeader")?,
                )
                .bind::<diesel::sql_types::Integer, _>(inv_id)
                .execute(&self.conn()?)?;
            Ok(())
        }
    }
}

mod impl_runs {
    use super::*;
    use crate::schema::runs::dsl::*;
    #[async_trait::async_trait]
    impl RunsRepo for DieselRepo {
        async fn run_new(&self, run_data: NewRun) -> Result<Run> {
            tokio::task::block_in_place(move || {
                diesel::insert_into(runs)
                    .values(&run_data)
                    .get_result(&self.conn()?)
                    .map_err(Into::into)
            })
        }

        async fn run_try_load(&self, run_id: i32) -> Result<Option<Run>> {
            tokio::task::block_in_place(move || {
                Ok(runs
                    .filter(id.eq(run_id))
                    .load::<Run>(&self.conn()?)?
                    .into_iter()
                    .next())
            })
        }

        async fn run_update(&self, run_id: RunId, patch: RunPatch) -> Result<()> {
            tokio::task::block_in_place(move || {
                diesel::update(runs)
                    .filter(id.eq(run_id))
                    .set(&patch)
                    .execute(&self.conn()?)
                    .map(|_| ())
                    .map_err(Into::into)
            })
        }

        async fn run_delete(&self, run_id: RunId) -> Result<()> {
            tokio::task::block_in_place(move || {
                diesel::delete(runs)
                    .filter(id.eq(run_id))
                    .execute(&self.conn()?)
                    .map(|_| ())
                    .map_err(Into::into)
            })
        }

        async fn run_select(
            &self,
            with_run_id: Option<RunId>,
            limit: Option<u32>,
        ) -> Result<Vec<Run>> {
            tokio::task::block_in_place(move || {
                let mut query = runs.into_boxed();

                if let Some(rid) = with_run_id {
                    query = query.filter(id.eq(rid));
                }
                let limit = limit.map(i64::from).unwrap_or(i64::max_value());
                Ok(query.limit(limit).load(&self.conn()?)?)
            })
        }
    }
}

mod impl_kv {
    use super::*;
    use crate::schema::kv::dsl::*;

    #[async_trait::async_trait]
    impl KvRepo for DieselRepo {
        async fn kv_get_raw(&self, key: &str) -> Result<Option<Vec<u8>>> {
            tokio::task::block_in_place(move || {
                let items: Vec<KvPair> = kv.filter(name.eq(key)).load(&self.conn()?)?;
                assert!(items.len() <= 1);
                Ok(items.into_iter().next().map(|item| item.value))
            })
        }

        async fn kv_put_raw(&self, key: &str, val: &[u8]) -> Result<()> {
            tokio::task::block_in_place(move || {
                let item = KvPair {
                    key: key.to_string(),
                    value: val.to_vec(),
                };
                diesel::insert_into(kv)
                    .values(item)
                    .execute(&self.conn()?)
                    .map_err(Into::into)
                    .map(drop)
            })
        }

        async fn kv_del(&self,key:&str) ->Result<()>{
            tokio::task::block_in_place(move||{
                diesel::delete(kv)
                .filter(name.eq(key))
                .execute(&self.conn()?)
                .map(drop)
                .map_err(Into::into)
            })
        }
    }
}

impl Repo for DieselRepo {}
