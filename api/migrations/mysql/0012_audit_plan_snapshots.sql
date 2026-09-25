-- Preserve item and relation before/after snapshots on existing audit rows.
ALTER TABLE audit ADD COLUMN details LONGTEXT NULL;
