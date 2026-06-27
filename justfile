set dotenv-load := true

# Rewrite inline insta snapshots.
rewrite:
    INSTA_UPDATE=always cargo test
