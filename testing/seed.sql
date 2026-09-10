-- Seed data for the local Postgres test instance.
--
-- Kept small and fully predictable: adapter tests assert on exact rows, and
-- docs/18-testing-strategy.md requires proving a preview returns the correct
-- matching records for a filter, which is only meaningful against known data.
--
-- Re-runnable. Every test that mutates data calls this to return to a known
-- state, so tests do not depend on execution order.

DROP TABLE IF EXISTS orders;
DROP TABLE IF EXISTS users;

CREATE TABLE users (
    id          integer PRIMARY KEY,
    email       text    NOT NULL UNIQUE,
    full_name   text    NOT NULL,
    signup_date date    NOT NULL,
    active      boolean NOT NULL DEFAULT true
);

INSERT INTO users (id, email, full_name, signup_date, active) VALUES
    (1, 'ada@example.com',     'Ada Lovelace',      DATE '2022-03-14', true),
    (2, 'grace@example.com',   'Grace Hopper',      DATE '2023-07-01', true),
    (3, 'alan@example.com',    'Alan Turing',       DATE '2023-11-23', false),
    (4, 'katherine@example.com','Katherine Johnson',DATE '2024-01-09', true),
    (5, 'margaret@example.com','Margaret Hamilton', DATE '2024-06-30', true),
    (6, 'barbara@example.com', 'Barbara Liskov',    DATE '2025-02-17', false),
    (7, 'donald@example.com',  'Donald Knuth',      DATE '2025-08-05', true);

CREATE TABLE orders (
    id       integer PRIMARY KEY,
    user_id  integer NOT NULL REFERENCES users (id),
    total    numeric(10, 2) NOT NULL,
    placed_at date   NOT NULL
);

INSERT INTO orders (id, user_id, total, placed_at) VALUES
    (1, 1, 42.00,  DATE '2024-02-01'),
    (2, 2, 130.50, DATE '2024-05-12'),
    (3, 2, 19.99,  DATE '2025-01-03'),
    (4, 5, 87.25,  DATE '2025-09-19');

-- A table with no other table depending on it, so schema-operation tests can
-- drop and truncate freely without tripping the orders foreign key.
DROP TABLE IF EXISTS disposable;

CREATE TABLE disposable (
    id    integer PRIMARY KEY,
    label text    NOT NULL,
    note  text
);

INSERT INTO disposable (id, label, note) VALUES
    (1, 'first',  NULL),
    (2, 'second', 'has a note'),
    (3, 'third',  NULL);
