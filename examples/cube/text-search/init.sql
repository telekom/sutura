CREATE DATABASE text_search;
CREATE TABLE text_search.notes (id String, body Nullable(String), cohort String)
ENGINE = MergeTree ORDER BY id;
INSERT INTO text_search.notes
SELECT toString(number),
    multiIf(number = 0, 'secret_other', number = 1, 'secret_TERM', number IN (2, 3), 'visible TERM',
        number = 4, NULL, number = 5, 'term', 'plain'),
    multiIf(number < 2, 'hidden', number < 7, 'rare', 'common')
FROM numbers(1000000);
