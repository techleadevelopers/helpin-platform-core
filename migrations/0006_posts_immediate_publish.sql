ALTER TABLE posts ALTER COLUMN moderation_status SET DEFAULT 'approved';
ALTER TABLE post_media ALTER COLUMN moderation_status SET DEFAULT 'approved';

UPDATE posts
SET moderation_status = 'approved'
WHERE moderation_status = 'queued';

UPDATE post_media
SET moderation_status = 'approved'
WHERE moderation_status = 'queued';
