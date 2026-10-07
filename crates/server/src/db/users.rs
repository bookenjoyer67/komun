use sqlx::PgPool;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub struct UserProfileRow {
    pub id: Uuid,
    pub display_name: String,
    pub bio: Option<String>,
    pub avatar_path: Option<String>,
    pub encryption_public_key: Option<Vec<u8>>,
    pub role: String,
    pub post_count: i64,
    pub verified_post_count: i64,
    pub endorsement_count: i64,
    /// The mean of this user's ratings to one decimal, or `None` when nobody has reviewed them.
    /// `None` and `0.0` are different facts: a new trader must not look like a rated-zero one.
    pub rating_avg: Option<f64>,
    pub rating_count: i64,
    /// Feeds the Regular giver badge only; it is never published as a number.
    pub give_count: i64,
    pub first_give_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_give_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_seen: chrono::DateTime<chrono::Utc>,
    pub profile_json: serde_json::Value,
}

pub async fn get_profile(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<UserProfileRow>, sqlx::Error> {
    sqlx::query_as::<_, UserProfileRow>(
        r#"SELECT
            u.id, u.display_name, u.bio, u.avatar_path,
            u.encryption_public_key,
            u.role, u.created_at, u.last_seen, u.profile_json,
            COALESCE(p.post_count, 0) as post_count,
            COALESCE(v.verified_count, 0) as verified_post_count,
            COALESCE(e.endorsement_count, 0) as endorsement_count,
            r.rating_avg,
            COALESCE(r.rating_count, 0) as rating_count,
            COALESCE(g.give_count, 0) as give_count,
            g.first_give_at,
            g.last_give_at
        FROM users u
        LEFT JOIN LATERAL (
            SELECT COUNT(*)::bigint as post_count FROM posts WHERE author_id = u.id
        ) p ON true
        LEFT JOIN LATERAL (
            SELECT COUNT(*)::bigint as verified_count FROM posts WHERE author_id = u.id AND verified_by IS NOT NULL
        ) v ON true
        LEFT JOIN LATERAL (
            SELECT COUNT(*)::bigint as endorsement_count FROM endorsements WHERE endorsee_id = u.id
        ) e ON true
        -- M3.4: the aggregate is computed from `deal_reviews` on every read rather than kept in a
        -- counter column on `users`. A counter is a second answer to the same question, and the
        -- only thing it can do that this cannot is disagree with the rows.
        --
        -- `AVG` over no rows is NULL, which is exactly the "not rated yet" the API returns as
        -- `null`; `COUNT` over no rows is 0. `::float8` after the rounding because NUMERIC needs a
        -- decimal crate to cross into Rust, and one decimal place of a 1-5 mean is a value a
        -- double represents exactly enough to print.
        LEFT JOIN LATERAL (
            SELECT ROUND(AVG(rating), 1)::float8 as rating_avg,
                   COUNT(*)::bigint as rating_count
            FROM deal_reviews WHERE reviewee_id = u.id
        ) r ON true
        -- Regular giver (D3) counts offer and resource posts, read from the rows like the rating
        -- above rather than from a counter. A public badge rests only on posts a visitor could
        -- have seen, so this must agree with `db::posts::publicly_visible`. The statuses are
        -- named, not excluded: a status added later earns nothing until someone lists it here.
        LEFT JOIN LATERAL (
            SELECT COUNT(*)::bigint as give_count,
                   MIN(created_at) as first_give_at,
                   MAX(created_at) as last_give_at
            FROM posts
            WHERE author_id = u.id
              AND kind IN ('offer', 'resource')
              AND status IN ('active', 'matched', 'fulfilled', 'expired')
              AND visibility = 'public'
        ) g ON true
        WHERE u.id = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}
