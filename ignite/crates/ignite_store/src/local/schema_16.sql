-- Schema version 16. Where each express pass ran: the working tree, the commit it was at and
-- the change's own files there. A landing whose rebase moved the tree to a new tree ID looks up
-- the tree's last pass by its path, and reuses it when the commits landed since that commit
-- touch none of the change's files and no build input.
--
-- Additive: new columns, empty on the rows written before them.

ALTER TABLE express_passes ADD COLUMN tree_path TEXT;
ALTER TABLE express_passes ADD COLUMN head TEXT;
ALTER TABLE express_passes ADD COLUMN changed TEXT;
