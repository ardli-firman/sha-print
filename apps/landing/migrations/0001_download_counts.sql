CREATE TABLE IF NOT EXISTS download_counts (
  day TEXT NOT NULL,
  package TEXT NOT NULL CHECK (package IN ('windows-exe','windows-msi','linux-deb','linux-rpm','linux-rpm-el10','linux-appimage','android-apk')),
  clicks INTEGER NOT NULL DEFAULT 0 CHECK (clicks >= 0),
  PRIMARY KEY (day, package)
) WITHOUT ROWID;
