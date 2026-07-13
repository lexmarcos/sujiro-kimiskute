use serenity::model::id::UserId;

#[derive(Clone)]
pub struct ResolvedTrack {
    pub id: String,
    pub title: String,
    pub webpage_url: String,
    pub duration_seconds: Option<u64>,
    pub channel_name: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Clone)]
pub struct QueuedTrack {
    pub track: ResolvedTrack,
    pub requested_by: UserId,
}
