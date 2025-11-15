-- Seed initial tags
INSERT INTO tags (tag) VALUES ('human') ON CONFLICT (tag) DO NOTHING;
INSERT INTO tags (tag) VALUES ('bot') ON CONFLICT (tag) DO NOTHING;
