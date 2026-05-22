ALTER TABLE posts ALTER COLUMN moderation_status SET DEFAULT 'approved';
ALTER TABLE post_media ALTER COLUMN moderation_status SET DEFAULT 'approved';

UPDATE posts
SET moderation_status = 'approved'
WHERE moderation_status IN ('queued', 'needs_review');

UPDATE post_media
SET moderation_status = 'approved'
WHERE moderation_status IN ('queued', 'needs_review');

ALTER TABLE posts
  DROP CONSTRAINT IF EXISTS posts_no_pending_visibility;

ALTER TABLE posts
  ADD CONSTRAINT posts_no_pending_visibility
  CHECK (moderation_status NOT IN ('queued', 'needs_review'));

ALTER TABLE post_media
  DROP CONSTRAINT IF EXISTS post_media_no_pending_visibility;

ALTER TABLE post_media
  ADD CONSTRAINT post_media_no_pending_visibility
  CHECK (moderation_status NOT IN ('queued', 'needs_review'));
