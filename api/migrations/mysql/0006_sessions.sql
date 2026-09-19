-- Browser sessions, kept where they can be ended.
--
-- The session used to live entirely in a sealed cookie: the access token
-- travelled with the person, and nothing on the server knew a session existed.
-- That kept every execution environment interchangeable, which mattered and
-- still does. But it also meant the session could not outlive one upstream
-- access token — roughly an hour — because there was nowhere safe to put a
-- refresh token. A rotating refresh token in a stateless cookie is a cookie
-- that, once stolen, renews itself forever with nothing able to stop it.
--
-- So the refresh token goes here instead, and the cookie carries only an
-- opaque id. This is not affinity: every environment reads the same shared
-- table, so a request still lands anywhere. What it buys is the thing a
-- stateless cookie cannot have — an end. Signing out deletes the row, and the
-- next request from that cookie finds nothing.
--
-- The stored envelope is sealed with the same keys the cookie used, so the
-- tokens are no more readable in a database dump than they were in a browser.
CREATE TABLE IF NOT EXISTS sessions(
  id VARCHAR(191) NOT NULL,
  -- Who it belongs to, so a person's sessions can be found and ended.
  actor VARCHAR(191) NOT NULL,
  -- The sealed envelope: access token, refresh token, selected tenant.
  envelope MEDIUMTEXT NOT NULL,
  -- When the session itself ends, regardless of any token inside it. A
  -- session that can be renewed indefinitely is one that never ends.
  expires_at VARCHAR(40) NOT NULL,
  created_at VARCHAR(40) NOT NULL,
  last_used_at VARCHAR(40) NOT NULL,
  PRIMARY KEY(id),
  KEY sessions_actor (actor),
  KEY sessions_expiry (expires_at)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
