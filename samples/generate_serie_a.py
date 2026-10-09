#!/usr/bin/env python3
"""Writes serie_a.postgres.sql and serie_a.mysql.sql: a Serie A 2025-26 sample
database for trying Savoia Studio by hand (explorer, ER diagram, console, big results).

Clubs, cities and stadiums are real (capacities and populations approximate).
Every person is invented, so squads never go stale. The data is the same in both
files; only the dialect differs. Run: python3 samples/generate_serie_a.py
"""

import random
from datetime import date, datetime, timedelta
from pathlib import Path

HERE = Path(__file__).parent
rng = random.Random(1898)  # first Italian championship

REGIONS = ["Lombardia", "Emilia-Romagna", "Sardegna", "Toscana", "Liguria", "Veneto",
           "Piemonte", "Lazio", "Puglia", "Campania", "Friuli-Venezia Giulia"]

# name, region, population (approx.), latitude, longitude
CITIES = [
    ("Bergamo", "Lombardia", 120000, 45.69830, 9.67730),
    ("Bologna", "Emilia-Romagna", 390000, 44.49490, 11.34260),
    ("Cagliari", "Sardegna", 149000, 39.22380, 9.12170),
    ("Como", "Lombardia", 83000, 45.80810, 9.08520),
    ("Cremona", "Lombardia", 71000, 45.13320, 10.02270),
    ("Firenze", "Toscana", 360000, 43.76960, 11.25580),
    ("Genova", "Liguria", 560000, 44.40560, 8.94630),
    ("Verona", "Veneto", 255000, 45.43840, 10.99160),
    ("Milano", "Lombardia", 1370000, 45.46420, 9.19000),
    ("Torino", "Piemonte", 850000, 45.07030, 7.68690),
    ("Roma", "Lazio", 2750000, 41.90280, 12.49640),
    ("Lecce", "Puglia", 94000, 40.35290, 18.17430),
    ("Napoli", "Campania", 910000, 40.85180, 14.26810),
    ("Parma", "Emilia-Romagna", 198000, 44.80150, 10.32790),
    ("Pisa", "Toscana", 89000, 43.72280, 10.40170),
    ("Reggio Emilia", "Emilia-Romagna", 171000, 44.69830, 10.63120),
    ("Sassuolo", "Emilia-Romagna", 40000, 44.54400, 10.78400),
    ("Udine", "Friuli-Venezia Giulia", 98000, 46.07110, 13.23460),
]

# name, city, capacity (approx.), opened, surface
STADIUMS = [
    ("Gewiss Stadium", "Bergamo", 24950, 1928, "hybrid"),
    ("Stadio Renato Dall'Ara", "Bologna", 36462, 1927, "grass"),
    ("Unipol Domus", "Cagliari", 16416, 2017, "hybrid"),
    ("Stadio Giuseppe Sinigaglia", "Como", 13602, 1927, "grass"),
    ("Stadio Giovanni Zini", "Cremona", 16003, 1929, "grass"),
    ("Stadio Artemio Franchi", "Firenze", 43147, 1931, "hybrid"),
    ("Stadio Luigi Ferraris", "Genova", 36599, 1911, "grass"),
    ("Stadio Marcantonio Bentegodi", "Verona", 31045, 1963, "grass"),
    ("Stadio Giuseppe Meazza (San Siro)", "Milano", 75817, 1926, "hybrid"),
    ("Allianz Stadium", "Torino", 41507, 2011, "hybrid"),
    ("Stadio Olimpico", "Roma", 70634, 1953, "hybrid"),
    ("Stadio Via del Mare", "Lecce", 31533, 1966, "grass"),
    ("Stadio Diego Armando Maradona", "Napoli", 54726, 1959, "hybrid"),
    ("Stadio Ennio Tardini", "Parma", 22352, 1923, "grass"),
    ("Arena Garibaldi", "Pisa", 9642, 1919, "grass"),
    ("Mapei Stadium", "Reggio Emilia", 21525, 1995, "hybrid"),
    ("Stadio Olimpico Grande Torino", "Torino", 27958, 1933, "grass"),
    ("Bluenergy Stadium", "Udine", 25144, 1976, "hybrid"),
]

# name, code, nickname, founded, city, stadium, colors
CLUBS = [
    ("Atalanta", "ATA", "La Dea", 1907, "Bergamo", "Gewiss Stadium", ["black", "blue"]),
    ("Bologna", "BOL", "Rossoblù", 1909, "Bologna", "Stadio Renato Dall'Ara", ["red", "blue"]),
    ("Cagliari", "CAG", "Isolani", 1920, "Cagliari", "Unipol Domus", ["red", "blue"]),
    ("Como", "COM", "Lariani", 1907, "Como", "Stadio Giuseppe Sinigaglia", ["blue", "white"]),
    ("Cremonese", "CRE", "Grigiorossi", 1903, "Cremona", "Stadio Giovanni Zini", ["grey", "red"]),
    ("Fiorentina", "FIO", "Viola", 1926, "Firenze", "Stadio Artemio Franchi", ["purple", "white"]),
    ("Genoa", "GEN", "Grifone", 1893, "Genova", "Stadio Luigi Ferraris", ["red", "blue"]),
    ("Hellas Verona", "VER", "Scaligeri", 1903, "Verona", "Stadio Marcantonio Bentegodi", ["yellow", "blue"]),
    ("Inter", "INT", "Nerazzurri", 1908, "Milano", "Stadio Giuseppe Meazza (San Siro)", ["black", "blue"]),
    ("Juventus", "JUV", "Bianconeri", 1897, "Torino", "Allianz Stadium", ["black", "white"]),
    ("Lazio", "LAZ", "Biancocelesti", 1900, "Roma", "Stadio Olimpico", ["sky blue", "white"]),
    ("Lecce", "LEC", "Salentini", 1908, "Lecce", "Stadio Via del Mare", ["yellow", "red"]),
    ("Milan", "MIL", "Rossoneri", 1899, "Milano", "Stadio Giuseppe Meazza (San Siro)", ["red", "black"]),
    ("Napoli", "NAP", "Partenopei", 1926, "Napoli", "Stadio Diego Armando Maradona", ["sky blue", "white"]),
    ("Parma", "PAR", "Crociati", 1913, "Parma", "Stadio Ennio Tardini", ["white", "black"]),
    ("Pisa", "PIS", "Nerazzurri", 1909, "Pisa", "Arena Garibaldi", ["black", "blue"]),
    ("Roma", "ROM", "Giallorossi", 1927, "Roma", "Stadio Olimpico", ["red", "yellow"]),
    ("Sassuolo", "SAS", "Neroverdi", 1920, "Sassuolo", "Mapei Stadium", ["green", "black"]),
    ("Torino", "TOR", "Granata", 1906, "Torino", "Stadio Olimpico Grande Torino", ["maroon", "white"]),
    ("Udinese", "UDI", "Friulani", 1896, "Udine", "Bluenergy Stadium", ["black", "white"]),
]

TROPHIES = [  # name, scope
    ("Serie A", "national"), ("Coppa Italia", "national"), ("Supercoppa Italiana", "national"),
    ("UEFA Champions League", "european"), ("UEFA Europa League", "european"),
    ("UEFA Europa Conference League", "european"),
]
# A few well-known honours, not a complete record.
HONOURS = [("Juventus", "Serie A", f"{y}-{(y + 1) % 100:02d}") for y in range(2011, 2020)] + [
    ("Inter", "Serie A", "2020-21"), ("Milan", "Serie A", "2021-22"), ("Napoli", "Serie A", "2022-23"),
    ("Inter", "Serie A", "2023-24"), ("Napoli", "Serie A", "2024-25"),
    ("Bologna", "Coppa Italia", "2024-25"), ("Inter", "UEFA Champions League", "2009-10"),
    ("Milan", "UEFA Champions League", "2006-07"), ("Juventus", "UEFA Champions League", "1995-96"),
    ("Atalanta", "UEFA Europa League", "2023-24"), ("Roma", "UEFA Europa Conference League", "2021-22"),
]

FIRST = ["Alessandro", "Andrea", "Antonio", "Cristian", "Daniele", "Davide", "Edoardo", "Emanuele",
         "Federico", "Filippo", "Francesco", "Gabriele", "Giacomo", "Gianluca", "Giorgio", "Giovanni",
         "Giuseppe", "Leonardo", "Lorenzo", "Luca", "Manuel", "Marco", "Matteo", "Mattia", "Michele",
         "Nicolò", "Paolo", "Pietro", "Riccardo", "Roberto", "Salvatore", "Samuele", "Simone",
         "Stefano", "Tommaso", "Valerio", "Vincenzo"]
LAST = ["Amato", "Barbieri", "Bellini", "Benedetti", "Bernardi", "Bianchi", "Bruno", "Caruso",
        "Cattaneo", "Colombo", "Conti", "Costa", "D'Angelo", "De Luca", "Esposito", "Fabbri",
        "Ferrari", "Ferraro", "Fontana", "Galli", "Gallo", "Gatti", "Giordano", "Grasso", "Greco",
        "Leone", "Lombardi", "Longo", "Mancini", "Marchetti", "Mariani", "Marino", "Martini",
        "Moretti", "Morelli", "Negri", "Orlando", "Pellegrini", "Rizzo", "Romano", "Rossi", "Russo",
        "Sala", "Santoro", "Serra", "Silvestri", "Testa", "Villa", "Vitale"]
# Players from abroad: first, last, nationality (ISO 3166-1 alpha-2)
ABROAD = [("Lucas", "Moreira", "BR"), ("Mateo", "Fernández", "AR"), ("Jonas", "Lindqvist", "SE"),
          ("Youssef", "Benali", "MA"), ("Kevin", "Dubois", "FR"), ("Luka", "Horvat", "HR"),
          ("Diego", "Ramírez", "UY"), ("Jakub", "Nowak", "PL"), ("Thomas", "Müller-Lang", "DE"),
          ("Ibrahima", "Diallo", "SN"), ("Nikola", "Petrović", "RS"), ("Rafael", "Santos", "PT"),
          ("Emil", "Hansen", "DK"), ("Kenji", "Watanabe", "JP"), ("Oliver", "O'Brien", "IE")]

SQUAD = [("goalkeeper", 3), ("defender", 8), ("midfielder", 8), ("forward", 5)]
SEASON_START = date(2025, 8, 24)
PLAYED_MATCHDAYS = 20  # later fixtures have no score yet (NULLs)


def person():
    if rng.random() < 0.3:
        return rng.choice(ABROAD)
    return rng.choice(FIRST), rng.choice(LAST), "IT"


def build():
    data = {}
    data["regions"] = [(i + 1, r) for i, r in enumerate(REGIONS)]
    region_id = {r: i for i, r in data["regions"]}
    data["cities"] = [(i + 1, n, region_id[r], p, lat, lon) for i, (n, r, p, lat, lon) in enumerate(CITIES)]
    city_id = {c[1]: c[0] for c in data["cities"]}
    data["stadiums"] = [(i + 1, n, city_id[c], cap, y, s) for i, (n, c, cap, y, s) in enumerate(STADIUMS)]
    stadium_id = {s[1]: s[0] for s in data["stadiums"]}
    stadium_capacity = {s[0]: s[3] for s in data["stadiums"]}
    data["clubs"] = [
        (i + 1, n, code, nick, f, city_id[c], stadium_id[s], colors,
         f"https://www.example.org/clubs/{code.lower()}")
        for i, (n, code, nick, f, c, s, colors) in enumerate(CLUBS)
    ]
    club_id = {c[1]: c[0] for c in data["clubs"]}
    club_stadium = {c[0]: c[6] for c in data["clubs"]}
    data["seasons"] = [(1, "2024-25", date(2024, 8, 17), date(2025, 5, 25)),
                       (2, "2025-26", date(2025, 8, 23), date(2026, 5, 24))]

    data["coaches"] = []
    data["directors"] = []
    for cid, *_ in data["clubs"]:
        first, last, nat = person()
        born = date(rng.randint(1960, 1985), rng.randint(1, 12), rng.randint(1, 28))
        data["coaches"].append((cid, first, last, born, nat, cid, date(rng.randint(2021, 2025), 7, 1)))
        base = len(data["directors"])
        for k, role in enumerate(["president", "chief executive", "sporting director", "team manager"]):
            first, last, _ = person()
            reports_to = None if k == 0 else base + k  # president ← CEO ← sporting director ← team manager
            since = date(rng.randint(2010, 2025), rng.randint(1, 12), 1)
            data["directors"].append((base + k + 1, cid, f"{first} {last}", role, since, reports_to))

    data["players"] = []
    data["contracts"] = []
    pid = 0
    squads = {}
    for cid, *_ in data["clubs"]:
        numbers = sorted(rng.sample(range(2, 100), 23))
        squads[cid] = []
        n = 0
        for position, count in SQUAD:
            for _ in range(count):
                pid += 1
                first, last, nat = person()
                born = date(rng.randint(1990, 2007), rng.randint(1, 12), rng.randint(1, 28))
                height = rng.randint(185, 198) if position == "goalkeeper" else rng.randint(168, 194)
                foot = rng.choices(["right", "left", "both"], [70, 25, 5])[0]
                value = rng.randint(2, 900) * 100_000
                shirt = 1 if n == 0 else numbers[n - 1]
                data["players"].append((pid, first, last, born, nat, position, shirt, cid, height, foot, value))
                squads[cid].append((pid, position))
                start = date(rng.randint(2019, 2025), 7, 1)
                end = date(start.year + rng.randint(2, 5), 6, 30)
                data["contracts"].append((pid, cid, start, end, rng.randint(3, 120) * 10_000))
                if rng.random() < 0.25:  # an earlier spell elsewhere
                    other = rng.choice([c for c in club_id.values() if c != cid])
                    data["contracts"].append(
                        (pid, other, date(start.year - 3, 7, 1), date(start.year, 6, 30), rng.randint(3, 60) * 10_000))
                n += 1
    for _ in range(10):  # free agents
        pid += 1
        first, last, nat = person()
        position = rng.choice(["defender", "midfielder", "forward"])
        born = date(rng.randint(1988, 2004), rng.randint(1, 12), rng.randint(1, 28))
        data["players"].append((pid, first, last, born, nat, position, None, None, rng.randint(170, 192), "right", None))

    # Double round robin by the circle method: every club once per matchday.
    clubs = list(club_id.values())
    rounds = []
    order = clubs[:]
    for _ in range(19):
        pairs = [(order[i], order[19 - i]) for i in range(10)]
        rounds.append(pairs)
        order = [order[0]] + [order[-1]] + order[1:-1]
    rounds = [[(h, a) if r % 2 == 0 else (a, h) for h, a in pairs] for r, pairs in enumerate(rounds)]
    rounds += [[(a, h) for h, a in pairs] for pairs in rounds]

    data["matches"] = []
    data["goals"] = []
    mid = gid = 0
    for md, pairs in enumerate(rounds, start=1):
        day = SEASON_START + timedelta(weeks=md - 1)
        for k, (home, away) in enumerate(pairs):
            mid += 1
            kickoff = datetime(day.year, day.month, day.day, [12, 15, 18, 20][k % 4], 30 if k % 4 == 0 else 0)
            stadium = club_stadium[home]
            if md <= PLAYED_MATCHDAYS:
                hg, ag = rng.choices(range(6), [22, 33, 25, 12, 6, 2])[0], rng.choices(range(5), [30, 35, 22, 9, 4])[0]
                attendance = int(stadium_capacity[stadium] * rng.uniform(0.55, 0.99))
            else:
                hg = ag = attendance = None
            data["matches"].append((mid, 2, md, home, away, stadium, kickoff, hg, ag, attendance))
            for club, count in ((home, hg or 0), (away, ag or 0)):
                for _ in range(count):
                    gid += 1
                    own = rng.random() < 0.04
                    scorer_club = away if own and club == home else home if own else club
                    weights = [{"goalkeeper": 0, "defender": 2, "midfielder": 4, "forward": 10}[p] for _, p in squads[scorer_club]]
                    scorer = rng.choices(squads[scorer_club], weights)[0][0]
                    minute = rng.randint(1, 90)
                    stoppage = rng.randint(1, 6) if minute in (45, 90) and rng.random() < 0.4 else 0
                    data["goals"].append((gid, mid, scorer, club, minute, stoppage, rng.random() < 0.1 and not own, own))

    data["trophies"] = [(i + 1, n, s) for i, (n, s) in enumerate(TROPHIES)]
    trophy_id = {t[1]: t[0] for t in data["trophies"]}
    data["club_honours"] = [(club_id[c], trophy_id[t], s) for c, t, s in HONOURS]
    return data


def lit(v, dialect):
    if v is None:
        return "NULL"
    if isinstance(v, bool):
        return ("TRUE" if v else "FALSE") if dialect == "pg" else ("1" if v else "0")
    if isinstance(v, (int, float)):
        return repr(v)
    if isinstance(v, datetime):
        return f"'{v:%Y-%m-%d %H:%M:%S}'"  # Europe/Rome local time
    if isinstance(v, date):
        return f"'{v:%Y-%m-%d}'"
    if isinstance(v, list):
        if dialect == "pg":
            return "ARRAY[" + ", ".join(lit(x, dialect) for x in v) + "]"
        return "JSON_ARRAY(" + ", ".join(lit(x, dialect) for x in v) + ")"
    s = str(v).replace("'", "''")
    return f"'{s}'"


def inserts(table, columns, rows, dialect, batch=200):
    out = []
    for i in range(0, len(rows), batch):
        values = ",\n  ".join("(" + ", ".join(lit(v, dialect) for v in row) + ")" for row in rows[i:i + batch])
        out.append(f"INSERT INTO {table} ({', '.join(columns)}) VALUES\n  {values};")
    return "\n".join(out)


COLUMNS = {
    "regions": ["id", "name"],
    "cities": ["id", "name", "region_id", "population", "latitude", "longitude"],
    "stadiums": ["id", "name", "city_id", "capacity", "opened_year", "surface"],
    "clubs": ["id", "name", "short_name", "nickname", "founded", "city_id", "stadium_id", "colors", "website"],
    "seasons": ["id", "label", "starts_on", "ends_on"],
    "coaches": ["id", "first_name", "last_name", "birth_date", "nationality", "club_id", "since"],
    "directors": ["id", "club_id", "full_name", "role", "since", "reports_to"],
    "players": ["id", "first_name", "last_name", "birth_date", "nationality", "position", "shirt_number",
                "club_id", "height_cm", "preferred_foot", "market_value_eur"],
    "contracts": ["player_id", "club_id", "starts_on", "ends_on", "salary_eur"],
    "matches": ["id", "season_id", "matchday", "home_club_id", "away_club_id", "stadium_id", "kickoff",
                "home_goals", "away_goals", "attendance"],
    "goals": ["id", "match_id", "player_id", "club_id", "minute", "stoppage", "is_penalty", "is_own_goal"],
    "trophies": ["id", "name", "scope"],
    "club_honours": ["club_id", "trophy_id", "season_label"],
}

PG_SCHEMA = """\
-- Serie A 2025-26 sample database for Savoia Studio (PostgreSQL 13+).
-- Generated by samples/generate_serie_a.py; edit that, not this file.
-- Real clubs and stadiums (approximate figures); every person is invented.
--   psql postgres://savoia:savoia@127.0.0.1:54317/savoia -f samples/serie_a.postgres.sql

DROP SCHEMA IF EXISTS serie_a CASCADE;
CREATE SCHEMA serie_a;
COMMENT ON SCHEMA serie_a IS 'Serie A 2025-26 sample data';
SET search_path = serie_a;

CREATE TYPE player_position AS ENUM ('goalkeeper', 'defender', 'midfielder', 'forward');
CREATE TYPE director_role AS ENUM ('president', 'chief executive', 'sporting director', 'team manager');

CREATE TABLE regions (
  id   smallint PRIMARY KEY,
  name text NOT NULL UNIQUE
);

CREATE TABLE cities (
  id         integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
  name       text NOT NULL UNIQUE,
  region_id  smallint NOT NULL REFERENCES regions (id),
  population integer CHECK (population > 0),
  latitude   numeric(8, 5),
  longitude  numeric(8, 5)
);

CREATE TABLE stadiums (
  id          integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
  name        text NOT NULL UNIQUE,
  city_id     integer NOT NULL REFERENCES cities (id),
  capacity    integer NOT NULL CHECK (capacity > 0),
  opened_year smallint,
  surface     text NOT NULL DEFAULT 'grass' CHECK (surface IN ('grass', 'hybrid', 'artificial'))
);

CREATE TABLE clubs (
  id         integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
  name       text NOT NULL UNIQUE,
  short_name char(3) NOT NULL UNIQUE,
  nickname   text,
  founded    smallint NOT NULL,
  city_id    integer NOT NULL REFERENCES cities (id),
  stadium_id integer NOT NULL REFERENCES stadiums (id),
  colors     text[] NOT NULL,
  website    text,
  badge      bytea
);
COMMENT ON COLUMN clubs.stadium_id IS 'Home ground; Inter/Milan and Lazio/Roma share one';

CREATE TABLE seasons (
  id        smallint PRIMARY KEY,
  label     varchar(7) NOT NULL UNIQUE,
  starts_on date NOT NULL,
  ends_on   date NOT NULL,
  CHECK (ends_on > starts_on)
);

CREATE TABLE coaches (
  id          integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
  first_name  text NOT NULL,
  last_name   text NOT NULL,
  birth_date  date NOT NULL,
  nationality char(2) NOT NULL,
  club_id     integer UNIQUE REFERENCES clubs (id),
  since       date
);

CREATE TABLE directors (
  id         integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
  club_id    integer NOT NULL REFERENCES clubs (id) ON DELETE CASCADE,
  full_name  text NOT NULL,
  role       director_role NOT NULL,
  since      date NOT NULL,
  reports_to integer REFERENCES directors (id),
  UNIQUE (club_id, role)
);

CREATE TABLE players (
  id               integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
  first_name       text NOT NULL,
  last_name        text NOT NULL,
  birth_date       date NOT NULL,
  nationality      char(2) NOT NULL,
  position         player_position NOT NULL,
  shirt_number     smallint CHECK (shirt_number BETWEEN 1 AND 99),
  club_id          integer REFERENCES clubs (id),
  height_cm        smallint,
  preferred_foot   text CHECK (preferred_foot IN ('right', 'left', 'both')),
  market_value_eur numeric(12, 0),
  UNIQUE (club_id, shirt_number)
);
CREATE INDEX players_last_name_lower ON players (lower(last_name));
CREATE INDEX players_club ON players (club_id);

CREATE TABLE contracts (
  player_id  integer NOT NULL REFERENCES players (id) ON DELETE CASCADE,
  club_id    integer NOT NULL REFERENCES clubs (id),
  starts_on  date NOT NULL,
  ends_on    date NOT NULL,
  salary_eur numeric(12, 2) NOT NULL,
  PRIMARY KEY (player_id, starts_on)
);

CREATE TABLE matches (
  id           integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
  season_id    smallint NOT NULL REFERENCES seasons (id),
  matchday     smallint NOT NULL CHECK (matchday BETWEEN 1 AND 38),
  home_club_id integer NOT NULL REFERENCES clubs (id),
  away_club_id integer NOT NULL REFERENCES clubs (id),
  stadium_id   integer NOT NULL REFERENCES stadiums (id),
  kickoff      timestamptz NOT NULL,
  home_goals   smallint,
  away_goals   smallint,
  attendance   integer,
  CHECK (home_club_id <> away_club_id),
  UNIQUE (season_id, home_club_id, away_club_id)
);
CREATE INDEX matches_kickoff ON matches (kickoff);

CREATE TABLE goals (
  id          bigint GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
  match_id    integer NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
  player_id   integer NOT NULL REFERENCES players (id),
  club_id     integer NOT NULL REFERENCES clubs (id),
  minute      smallint NOT NULL CHECK (minute BETWEEN 1 AND 120),
  stoppage    smallint NOT NULL DEFAULT 0,
  is_penalty  boolean NOT NULL DEFAULT false,
  is_own_goal boolean NOT NULL DEFAULT false
);
COMMENT ON COLUMN goals.club_id IS 'The club credited with the goal (for own goals, the opponent of the scorer)';
CREATE INDEX goals_player ON goals (player_id);

CREATE TABLE trophies (
  id    smallint PRIMARY KEY,
  name  text NOT NULL UNIQUE,
  scope text NOT NULL CHECK (scope IN ('national', 'european', 'world'))
);

CREATE TABLE club_honours (
  club_id      integer NOT NULL REFERENCES clubs (id),
  trophy_id    smallint NOT NULL REFERENCES trophies (id),
  season_label varchar(7) NOT NULL,
  PRIMARY KEY (club_id, trophy_id, season_label)
);

-- Many rows, for paging, streaming and cancel.
CREATE TABLE match_events (
  id        bigint GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
  match_id  integer NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
  minute    smallint NOT NULL,
  kind      text NOT NULL,
  player_id integer REFERENCES players (id),
  details   jsonb
);
CREATE INDEX match_events_match ON match_events (match_id);

CREATE SEQUENCE ticket_number_seq START 100000;
"""

PG_TAIL = """\
-- Identity columns were filled with explicit ids; move them past the data.
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['cities', 'stadiums', 'clubs', 'coaches', 'directors', 'players', 'matches', 'goals'] LOOP
    EXECUTE format('SELECT setval(pg_get_serial_sequence(%L, ''id''), (SELECT max(id) FROM serie_a.%I))', 'serie_a.' || t, t);
  END LOOP;
END $$;

INSERT INTO match_events (match_id, minute, kind, player_id, details)
SELECT m.id,
       1 + (g * 7) % 95,
       (ARRAY['pass', 'shot', 'foul', 'corner', 'offside', 'save', 'tackle', 'yellow card'])[1 + g % 8],
       -- Each club's 24 players have consecutive ids.
       (CASE WHEN g % 2 = 0 THEN m.home_club_id ELSE m.away_club_id END - 1) * 24 + 1 + g % 24,
       jsonb_build_object('x', (g * 13) % 105, 'y', (g * 29) % 68, 'seq', g)
FROM generate_series(1, 150000) g
JOIN matches m ON m.id = 1 + g % 200;

CREATE VIEW standings AS
WITH results AS (
  SELECT season_id, home_club_id AS club_id, home_goals AS gf, away_goals AS ga FROM matches WHERE home_goals IS NOT NULL
  UNION ALL
  SELECT season_id, away_club_id, away_goals, home_goals FROM matches WHERE home_goals IS NOT NULL
)
SELECT s.label AS season, c.name AS club,
       count(*) AS played,
       count(*) FILTER (WHERE gf > ga) AS won,
       count(*) FILTER (WHERE gf = ga) AS drawn,
       count(*) FILTER (WHERE gf < ga) AS lost,
       sum(gf) AS goals_for, sum(ga) AS goals_against, sum(gf - ga) AS goal_difference,
       sum(CASE WHEN gf > ga THEN 3 WHEN gf = ga THEN 1 ELSE 0 END) AS points
FROM results r JOIN clubs c ON c.id = r.club_id JOIN seasons s ON s.id = r.season_id
GROUP BY s.label, c.name
ORDER BY points DESC, goal_difference DESC, goals_for DESC;

CREATE VIEW top_scorers AS
SELECT p.first_name || ' ' || p.last_name AS player, c.name AS club,
       count(*) AS goals, count(*) FILTER (WHERE g.is_penalty) AS penalties
FROM goals g JOIN players p ON p.id = g.player_id JOIN clubs c ON c.id = p.club_id
WHERE NOT g.is_own_goal
GROUP BY p.id, p.first_name, p.last_name, c.name
ORDER BY goals DESC, player;

CREATE MATERIALIZED VIEW squad_values AS
SELECT c.name AS club, count(p.id) AS players, sum(p.market_value_eur) AS total_value_eur,
       round(avg(extract(year FROM age(date '2026-01-01', p.birth_date))), 1) AS average_age
FROM clubs c JOIN players p ON p.club_id = c.id
GROUP BY c.name;

CREATE FUNCTION points(p_club integer, p_season integer) RETURNS integer
LANGUAGE sql STABLE AS $$
  SELECT coalesce(sum(CASE WHEN gf > ga THEN 3 WHEN gf = ga THEN 1 ELSE 0 END), 0)::integer
  FROM (
    SELECT home_goals AS gf, away_goals AS ga FROM serie_a.matches
    WHERE season_id = p_season AND home_club_id = p_club AND home_goals IS NOT NULL
    UNION ALL
    SELECT away_goals, home_goals FROM serie_a.matches
    WHERE season_id = p_season AND away_club_id = p_club AND home_goals IS NOT NULL
  ) r
$$;

CREATE PROCEDURE promote_reserve(p_player integer, p_shirt smallint)
LANGUAGE sql AS $$
  UPDATE serie_a.players SET shirt_number = p_shirt WHERE id = p_player
$$;

ANALYZE;
"""

MY_SCHEMA = """\
-- Serie A 2025-26 sample database for Savoia Studio (MySQL 8.0.16+).
-- Generated by samples/generate_serie_a.py; edit that, not this file.
-- Real clubs and stadiums (approximate figures); every person is invented.
-- Creates the `serie_a` database, so run it as root:
--   docker compose exec -T mysql-8.4 mysql -uroot -psavoia < samples/serie_a.mysql.sql

DROP DATABASE IF EXISTS serie_a;
CREATE DATABASE serie_a CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci;
GRANT ALL PRIVILEGES ON serie_a.* TO 'savoia'@'%';
USE serie_a;

CREATE TABLE regions (
  id   SMALLINT PRIMARY KEY,
  name VARCHAR(60) NOT NULL UNIQUE
);

CREATE TABLE cities (
  id         INT AUTO_INCREMENT PRIMARY KEY,
  name       VARCHAR(80) NOT NULL UNIQUE,
  region_id  SMALLINT NOT NULL,
  population INT UNSIGNED,
  latitude   DECIMAL(8, 5),
  longitude  DECIMAL(8, 5),
  CONSTRAINT cities_region FOREIGN KEY (region_id) REFERENCES regions (id)
);

CREATE TABLE stadiums (
  id          INT AUTO_INCREMENT PRIMARY KEY,
  name        VARCHAR(80) NOT NULL UNIQUE,
  city_id     INT NOT NULL,
  capacity    INT UNSIGNED NOT NULL,
  opened_year YEAR,
  surface     ENUM('grass', 'hybrid', 'artificial') NOT NULL DEFAULT 'grass',
  CONSTRAINT stadiums_city FOREIGN KEY (city_id) REFERENCES cities (id)
);

CREATE TABLE clubs (
  id         INT AUTO_INCREMENT PRIMARY KEY,
  name       VARCHAR(60) NOT NULL UNIQUE,
  short_name CHAR(3) NOT NULL UNIQUE,
  nickname   VARCHAR(60),
  founded    SMALLINT NOT NULL,
  city_id    INT NOT NULL,
  stadium_id INT NOT NULL COMMENT 'Home ground; Inter/Milan and Lazio/Roma share one',
  colors     JSON NOT NULL,
  website    VARCHAR(200),
  badge      BLOB,
  CONSTRAINT clubs_city FOREIGN KEY (city_id) REFERENCES cities (id),
  CONSTRAINT clubs_stadium FOREIGN KEY (stadium_id) REFERENCES stadiums (id)
);

CREATE TABLE seasons (
  id        SMALLINT PRIMARY KEY,
  label     VARCHAR(7) NOT NULL UNIQUE,
  starts_on DATE NOT NULL,
  ends_on   DATE NOT NULL,
  CHECK (ends_on > starts_on)
);

CREATE TABLE coaches (
  id          INT AUTO_INCREMENT PRIMARY KEY,
  first_name  VARCHAR(60) NOT NULL,
  last_name   VARCHAR(60) NOT NULL,
  birth_date  DATE NOT NULL,
  nationality CHAR(2) NOT NULL,
  club_id     INT UNIQUE,
  since       DATE,
  CONSTRAINT coaches_club FOREIGN KEY (club_id) REFERENCES clubs (id)
);

CREATE TABLE directors (
  id         INT AUTO_INCREMENT PRIMARY KEY,
  club_id    INT NOT NULL,
  full_name  VARCHAR(120) NOT NULL,
  role       ENUM('president', 'chief executive', 'sporting director', 'team manager') NOT NULL,
  since      DATE NOT NULL,
  reports_to INT,
  UNIQUE KEY directors_club_role (club_id, role),
  CONSTRAINT directors_club FOREIGN KEY (club_id) REFERENCES clubs (id) ON DELETE CASCADE,
  CONSTRAINT directors_reports_to FOREIGN KEY (reports_to) REFERENCES directors (id)
);

CREATE TABLE players (
  id               INT AUTO_INCREMENT PRIMARY KEY,
  first_name       VARCHAR(60) NOT NULL,
  last_name        VARCHAR(60) NOT NULL,
  birth_date       DATE NOT NULL,
  nationality      CHAR(2) NOT NULL,
  position         ENUM('goalkeeper', 'defender', 'midfielder', 'forward') NOT NULL,
  shirt_number     TINYINT UNSIGNED CHECK (shirt_number BETWEEN 1 AND 99),
  club_id          INT,
  height_cm        SMALLINT UNSIGNED,
  preferred_foot   ENUM('right', 'left', 'both'),
  market_value_eur DECIMAL(12, 0),
  UNIQUE KEY players_club_shirt (club_id, shirt_number),
  KEY players_last_name_lower ((lower(last_name))),
  CONSTRAINT players_club FOREIGN KEY (club_id) REFERENCES clubs (id)
);

CREATE TABLE contracts (
  player_id  INT NOT NULL,
  club_id    INT NOT NULL,
  starts_on  DATE NOT NULL,
  ends_on    DATE NOT NULL,
  salary_eur DECIMAL(12, 2) NOT NULL,
  PRIMARY KEY (player_id, starts_on),
  CONSTRAINT contracts_player FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE,
  CONSTRAINT contracts_club FOREIGN KEY (club_id) REFERENCES clubs (id)
);

CREATE TABLE matches (
  id           INT AUTO_INCREMENT PRIMARY KEY,
  season_id    SMALLINT NOT NULL,
  matchday     TINYINT UNSIGNED NOT NULL CHECK (matchday BETWEEN 1 AND 38),
  home_club_id INT NOT NULL,
  away_club_id INT NOT NULL,
  stadium_id   INT NOT NULL,
  kickoff      DATETIME NOT NULL COMMENT 'Local time (Europe/Rome)',
  home_goals   TINYINT UNSIGNED,
  away_goals   TINYINT UNSIGNED,
  attendance   INT UNSIGNED,
  CHECK (home_club_id <> away_club_id),
  UNIQUE KEY matches_pairing (season_id, home_club_id, away_club_id),
  KEY matches_kickoff (kickoff),
  CONSTRAINT matches_season FOREIGN KEY (season_id) REFERENCES seasons (id),
  CONSTRAINT matches_home FOREIGN KEY (home_club_id) REFERENCES clubs (id),
  CONSTRAINT matches_away FOREIGN KEY (away_club_id) REFERENCES clubs (id),
  CONSTRAINT matches_stadium FOREIGN KEY (stadium_id) REFERENCES stadiums (id)
);

CREATE TABLE goals (
  id          BIGINT AUTO_INCREMENT PRIMARY KEY,
  match_id    INT NOT NULL,
  player_id   INT NOT NULL,
  club_id     INT NOT NULL COMMENT 'The club credited with the goal (for own goals, the opponent of the scorer)',
  minute      TINYINT UNSIGNED NOT NULL CHECK (minute BETWEEN 1 AND 120),
  stoppage    TINYINT UNSIGNED NOT NULL DEFAULT 0,
  is_penalty  BOOLEAN NOT NULL DEFAULT FALSE,
  is_own_goal BOOLEAN NOT NULL DEFAULT FALSE,
  CONSTRAINT goals_match FOREIGN KEY (match_id) REFERENCES matches (id) ON DELETE CASCADE,
  CONSTRAINT goals_player FOREIGN KEY (player_id) REFERENCES players (id),
  CONSTRAINT goals_club FOREIGN KEY (club_id) REFERENCES clubs (id)
);

CREATE TABLE trophies (
  id    SMALLINT PRIMARY KEY,
  name  VARCHAR(60) NOT NULL UNIQUE,
  scope ENUM('national', 'european', 'world') NOT NULL
);

CREATE TABLE club_honours (
  club_id      INT NOT NULL,
  trophy_id    SMALLINT NOT NULL,
  season_label VARCHAR(7) NOT NULL,
  PRIMARY KEY (club_id, trophy_id, season_label),
  CONSTRAINT club_honours_club FOREIGN KEY (club_id) REFERENCES clubs (id),
  CONSTRAINT club_honours_trophy FOREIGN KEY (trophy_id) REFERENCES trophies (id)
);

-- Many rows, for paging, streaming and cancel.
CREATE TABLE match_events (
  id        BIGINT AUTO_INCREMENT PRIMARY KEY,
  match_id  INT NOT NULL,
  minute    TINYINT UNSIGNED NOT NULL,
  kind      VARCHAR(20) NOT NULL,
  player_id INT,
  details   JSON,
  CONSTRAINT match_events_match FOREIGN KEY (match_id) REFERENCES matches (id) ON DELETE CASCADE,
  CONSTRAINT match_events_player FOREIGN KEY (player_id) REFERENCES players (id)
);
"""

MY_TAIL = """\
SET SESSION cte_max_recursion_depth = 200000;
INSERT INTO match_events (match_id, minute, kind, player_id, details)
WITH RECURSIVE seq (g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM seq WHERE g < 150000)
SELECT m.id,
       1 + (g * 7) % 95,
       ELT(1 + g % 8, 'pass', 'shot', 'foul', 'corner', 'offside', 'save', 'tackle', 'yellow card'),
       -- Each club's 24 players have consecutive ids.
       (IF(g % 2 = 0, m.home_club_id, m.away_club_id) - 1) * 24 + 1 + g % 24,
       JSON_OBJECT('x', (g * 13) % 105, 'y', (g * 29) % 68, 'seq', g)
FROM seq JOIN matches m ON m.id = 1 + g % 200;

CREATE VIEW standings AS
WITH results AS (
  SELECT season_id, home_club_id AS club_id, home_goals AS gf, away_goals AS ga FROM matches WHERE home_goals IS NOT NULL
  UNION ALL
  SELECT season_id, away_club_id, away_goals, home_goals FROM matches WHERE home_goals IS NOT NULL
)
SELECT s.label AS season, c.name AS club,
       COUNT(*) AS played,
       SUM(gf > ga) AS won, SUM(gf = ga) AS drawn, SUM(gf < ga) AS lost,
       SUM(gf) AS goals_for, SUM(ga) AS goals_against, SUM(CAST(gf AS SIGNED) - CAST(ga AS SIGNED)) AS goal_difference,
       SUM(CASE WHEN gf > ga THEN 3 WHEN gf = ga THEN 1 ELSE 0 END) AS points
FROM results r JOIN clubs c ON c.id = r.club_id JOIN seasons s ON s.id = r.season_id
GROUP BY s.label, c.name
ORDER BY points DESC, goal_difference DESC, goals_for DESC;

CREATE VIEW top_scorers AS
SELECT CONCAT(p.first_name, ' ', p.last_name) AS player, c.name AS club,
       COUNT(*) AS goals, SUM(g.is_penalty) AS penalties
FROM goals g JOIN players p ON p.id = g.player_id JOIN clubs c ON c.id = p.club_id
WHERE NOT g.is_own_goal
GROUP BY p.id, p.first_name, p.last_name, c.name
ORDER BY goals DESC, player;

CREATE VIEW squad_values AS
SELECT c.name AS club, COUNT(p.id) AS players, SUM(p.market_value_eur) AS total_value_eur,
       ROUND(AVG(TIMESTAMPDIFF(YEAR, p.birth_date, '2026-01-01')), 1) AS average_age
FROM clubs c JOIN players p ON p.club_id = c.id
GROUP BY c.name;

DELIMITER //
CREATE FUNCTION points(p_club INT, p_season INT) RETURNS INT
READS SQL DATA DETERMINISTIC
BEGIN
  RETURN (
    SELECT COALESCE(SUM(CASE WHEN gf > ga THEN 3 WHEN gf = ga THEN 1 ELSE 0 END), 0)
    FROM (
      SELECT home_goals AS gf, away_goals AS ga FROM matches
      WHERE season_id = p_season AND home_club_id = p_club AND home_goals IS NOT NULL
      UNION ALL
      SELECT away_goals, home_goals FROM matches
      WHERE season_id = p_season AND away_club_id = p_club AND home_goals IS NOT NULL
    ) r
  );
END //

CREATE PROCEDURE promote_reserve(p_player INT, p_shirt TINYINT UNSIGNED)
MODIFIES SQL DATA
BEGIN
  UPDATE players SET shirt_number = p_shirt WHERE id = p_player;
END //
DELIMITER ;

ANALYZE TABLE players, matches, goals, match_events;
"""

ORDER = ["regions", "cities", "stadiums", "clubs", "seasons", "coaches", "directors", "players",
         "contracts", "matches", "goals", "trophies", "club_honours"]


def write(dialect, head, tail, path):
    rng.seed(1898)  # the same data for both dialects
    data = build()
    body = "\n\n".join(inserts(t, COLUMNS[t], data[t], dialect) for t in ORDER)
    if dialect == "pg":
        # Kickoffs are written in local time.
        body = "BEGIN;\nSET LOCAL TIME ZONE 'Europe/Rome';\n" + body + "\nCOMMIT;"
    path.write_text(head + "\n" + body + "\n\n" + tail)
    print(f"{path.name}: " + ", ".join(f"{len(data[t])} {t}" for t in ORDER))


write("pg", PG_SCHEMA, PG_TAIL, HERE / "serie_a.postgres.sql")
write("mysql", MY_SCHEMA, MY_TAIL, HERE / "serie_a.mysql.sql")
