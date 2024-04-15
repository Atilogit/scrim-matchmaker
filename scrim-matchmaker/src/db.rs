use std::str::FromStr;

use poise::serenity_prelude::futures::{StreamExt, TryStreamExt};
use sqlx::{postgres::PgRow, Row};

use crate::{
    lfs::{LookingForScrim, RankRange},
    Context, Error,
};

#[tracing::instrument(err, skip(ctx))]
pub async fn set_timezone(
    ctx: Context<'_>,
    zone: chrono_tz::Tz,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query("INSERT INTO users (id, timezone) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET timezone = $2")
        .bind(ctx.author().id.get() as i64)
        .bind(zone.name())
        .execute(&ctx.data().db)
        .await
}

#[tracing::instrument(err, skip(ctx))]
pub async fn get_timezone(ctx: Context<'_>) -> Result<chrono_tz::Tz, Error> {
    let row: (String,) = sqlx::query_as("SELECT timezone FROM users WHERE id = $1")
        .bind(ctx.author().id.get() as i64)
        .fetch_one(&ctx.data().db)
        .await
        .map_err(|_| {
            "You haven't set your timezone yet. Use `/timezone zone:<timezone>` to set it"
                .to_owned()
        })?;
    Ok(chrono_tz::Tz::from_str(&row.0).map_err(|_| "Invalid timezone")?)
}

#[tracing::instrument(err, skip(ctx))]
pub async fn create_scrim(ctx: Context<'_>, lfs: LookingForScrim) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO scrims (
                creator_id, region, platform, rank_from, rank_to, time, match_id, team_name, cancelled
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(lfs.creator_id)
    .bind(format!("{:?}", lfs.region))
    .bind(format!("{:?}", lfs.platform))
    .bind(lfs.range.0.start as i32)
    .bind(lfs.range.0.end as i32)
    .bind(lfs.time)
    .bind(lfs.match_id)
    .bind(lfs.team_name)
    .bind(lfs.cancelled)
    .execute(&ctx.data().db)
    .await?;

    Ok(())
}

#[tracing::instrument(err, skip(ctx))]
pub async fn cancel_scrim(ctx: Context<'_>, id: i32) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE scrims SET cancelled = TRUE WHERE id = $1")
        .bind(id)
        .execute(&ctx.data().db)
        .await?;
    Ok(())
}

#[tracing::instrument(err, skip(ctx))]
pub async fn restore_scrim(ctx: Context<'_>, id: i32) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE scrims SET cancelled = FALSE WHERE id = $1")
        .bind(id)
        .execute(&ctx.data().db)
        .await?;
    Ok(())
}

#[tracing::instrument(err, skip(ctx))]
pub async fn revoke_scrim(ctx: Context<'_>, id: i32) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE scrims SET match_id = NULL WHERE id = $1")
        .bind(id)
        .execute(&ctx.data().db)
        .await?;
    Ok(())
}

#[tracing::instrument(err, skip(ctx))]
pub async fn get_future_scrims(ctx: Context<'_>) -> Result<Vec<LookingForScrim>, sqlx::Error> {
    sqlx::query("SELECT * FROM scrims WHERE creator_id = $1 AND time >= NOW() AND NOT cancelled")
        .bind(ctx.author().id.get() as i64)
        .fetch(&ctx.data().db)
        .map(|row| row.map(row_to_lfs))
        .try_collect()
        .await
}

pub async fn get_scrim(ctx: Context<'_>, id: i32) -> Result<LookingForScrim, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM scrims WHERE id = $1")
        .bind(id)
        .fetch_one(&ctx.data().db)
        .await?;
    Ok(row_to_lfs(row))
}

/// Match two scrims together. Only the first one with is updated, the second one is left as is.
/// If the second scrim is available, it will be returned.
pub async fn match_scrims(ctx: Context<'_>, id: i32, to: i32) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE scrims SET match_id = $2 WHERE id = $1")
        .bind(id)
        .bind(to)
        .execute(&ctx.data().db)
        .await?;
    Ok(())
}

fn row_to_lfs(row: PgRow) -> LookingForScrim {
    LookingForScrim {
        id: row.get("id"),
        creator_id: row.get::<i64, _>("creator_id"),
        team_name: row.get("team_name"),
        region: row.get::<&str, _>("region").parse().unwrap(),
        platform: row.get::<&str, _>("platform").parse().unwrap(),
        range: RankRange(
            row.get::<i32, _>("rank_from") as u32..row.get::<i32, _>("rank_to") as u32,
        ),
        time: row.get("time"),
        match_id: row.get("match_id"),
        cancelled: row.get("cancelled"),
    }
}

#[tracing::instrument(err, skip(ctx))]
pub async fn find_matches(
    ctx: Context<'_>,
    lfs: &LookingForScrim,
) -> Result<Vec<(f32, LookingForScrim)>, sqlx::Error> {
    let rank_weight = 1;
    let time_weight = 1. / 3600. * 500.;
    let region_weight = 500;
    let platform_weight = 200;
    sqlx::query(
        "SELECT *,
        (
            ABS((rank_from + rank_to) / 2 - $1) * $2 +
            ABS(EXTRACT(epoch FROM time - $3)) * $4 +
            (region != $5)::INT * $6 +
            (platform != $7)::INT * $8 +

            (NOT match_id IS NULL AND match_id = 7)::INT * -10000000
        )::FLOAT4 AS difference
        FROM scrims
        WHERE creator_id != $10 AND time >= NOW() AND NOT cancelled AND (match_id IS NULL OR match_id = $9)
        ORDER BY difference ASC LIMIT 5
        ",
    )
    .bind(((lfs.range.0.start + lfs.range.0.end) / 2) as i32)
    .bind(rank_weight)
    .bind(lfs.time)
    .bind(time_weight)
    .bind(format!("{:?}", lfs.region))
    .bind(region_weight)
    .bind(format!("{:?}", lfs.platform))
    .bind(platform_weight)
    .bind(lfs.id)
    .bind(lfs.creator_id)
    .fetch(&ctx.data().db)
    .map(|row| row.map(|row| (row.get("difference"), row_to_lfs(row))))
    .try_collect()
    .await
}
