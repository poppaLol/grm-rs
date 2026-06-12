import asyncio
import os
import time
from typing import Literal

from grm_rs import AsyncNeo4jSession, FieldDefinition


def require_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"set {name} to run the Python Neo4j movies smoke test")
    return value


def field(
    name: str,
    value_type: Literal["string", "int", "float", "bool"] = "string",
    required: bool = True,
) -> FieldDefinition:
    return FieldDefinition(name=name, type=value_type, required=required)


MOVIES = [
    {
        "key": "the_matrix",
        "title": "The Matrix",
        "released": 1999,
        "tagline": "Welcome to the Real World",
        "cast": [
            ("Keanu Reeves", 1964, "Neo"),
            ("Carrie-Anne Moss", 1967, "Trinity"),
            ("Laurence Fishburne", 1961, "Morpheus"),
            ("Hugo Weaving", 1960, "Agent Smith"),
            ("Emil Eifrem", 1978, "Emil"),
        ],
        "directors": [("Lilly Wachowski", 1967), ("Lana Wachowski", 1965)],
        "producers": [("Joel Silver", 1952)],
        "writers": [],
    },
    {
        "key": "matrix_reloaded",
        "title": "The Matrix Reloaded",
        "released": 2003,
        "tagline": "Free your mind",
        "cast": [
            ("Keanu Reeves", 1964, "Neo"),
            ("Carrie-Anne Moss", 1967, "Trinity"),
            ("Laurence Fishburne", 1961, "Morpheus"),
            ("Hugo Weaving", 1960, "Agent Smith"),
        ],
        "directors": [("Lilly Wachowski", 1967), ("Lana Wachowski", 1965)],
        "producers": [("Joel Silver", 1952)],
        "writers": [],
    },
    {
        "key": "devils_advocate",
        "title": "The Devil's Advocate",
        "released": 1997,
        "tagline": "Evil has its winning ways",
        "cast": [
            ("Keanu Reeves", 1964, "Kevin Lomax"),
            ("Charlize Theron", 1975, "Mary Ann Lomax"),
            ("Al Pacino", 1940, "John Milton"),
        ],
        "directors": [("Taylor Hackford", 1944)],
        "producers": [],
        "writers": [],
    },
    {
        "key": "few_good_men",
        "title": "A Few Good Men",
        "released": 1992,
        "tagline": "In the heart of the nation's capital, one man will stop at nothing.",
        "cast": [
            ("Tom Cruise", 1962, "Lt. Daniel Kaffee"),
            ("Jack Nicholson", 1937, "Col. Nathan R. Jessup"),
            ("Demi Moore", 1962, "Lt. Cdr. JoAnne Galloway"),
            ("Kevin Bacon", 1958, "Capt. Jack Ross"),
            ("Kiefer Sutherland", 1966, "Lt. Jonathan Kendrick"),
            ("Noah Wyle", 1971, "Cpl. Jeffrey Barnes"),
            ("Cuba Gooding Jr.", 1968, "Cpl. Carl Hammaker"),
            ("Kevin Pollak", 1957, "Lt. Sam Weinberg"),
            ("J.T. Walsh", 1943, "Lt. Col. Matthew Andrew Markinson"),
            ("James Marshall", 1967, "Pfc. Louden Downey"),
            ("Christopher Guest", 1948, "Dr. Stone"),
            ("Aaron Sorkin", 1961, "Man in Bar"),
        ],
        "directors": [("Rob Reiner", 1947)],
        "producers": [],
        "writers": [("Aaron Sorkin", 1961)],
    },
    {
        "key": "top_gun",
        "title": "Top Gun",
        "released": 1986,
        "tagline": "I feel the need, the need for speed.",
        "cast": [
            ("Tom Cruise", 1962, "Maverick"),
            ("Kelly McGillis", 1957, "Charlie"),
            ("Val Kilmer", 1959, "Iceman"),
            ("Anthony Edwards", 1962, "Goose"),
            ("Tom Skerritt", 1933, "Viper"),
            ("Meg Ryan", 1961, "Carole"),
        ],
        "directors": [("Tony Scott", 1944)],
        "producers": [],
        "writers": [("Jim Cash", 1941)],
    },
    {
        "key": "jerry_maguire",
        "title": "Jerry Maguire",
        "released": 2000,
        "tagline": "The rest of his life begins now.",
        "cast": [
            ("Tom Cruise", 1962, "Jerry Maguire"),
            ("Cuba Gooding Jr.", 1968, "Rod Tidwell"),
            ("Renee Zellweger", 1969, "Dorothy Boyd"),
            ("Kelly Preston", 1962, "Avery Bishop"),
            ("Jerry O'Connell", 1974, "Frank Cushman"),
            ("Jay Mohr", 1970, "Bob Sugar"),
            ("Bonnie Hunt", 1961, "Laurel Boyd"),
            ("Regina King", 1971, "Marcee Tidwell"),
            ("Jonathan Lipnicki", 1996, "Ray Boyd"),
        ],
        "directors": [("Cameron Crowe", 1957)],
        "producers": [("Cameron Crowe", 1957)],
        "writers": [("Cameron Crowe", 1957)],
    },
]

REVIEWS = [
    ("Jessica Thompson", "The Matrix", "An amazing journey", 95),
    ("James Thompson", "The Matrix", "Silly, but fun", 65),
    ("Angela Scope", "Jerry Maguire", "The greatest sports movie ever", 90),
]


async def define_schema(session: AsyncNeo4jSession) -> None:
    await session.model_create(
        "Person",
        "personId",
        [
            field("name"),
            field("born", "int", required=False),
            field("kind"),
            field("dataset"),
        ],
    )
    await session.model_create(
        "Movie",
        "movieId",
        [
            field("title"),
            field("released", "int"),
            field("tagline", required=False),
            field("dataset"),
        ],
    )
    await session.link_create(
        "ACTED_IN",
        "Person",
        "Movie",
        "actedInId",
        [field("role"), field("dataset")],
    )
    await session.link_create(
        "DIRECTED",
        "Person",
        "Movie",
        "directedId",
        [field("dataset")],
    )
    await session.link_create(
        "PRODUCED",
        "Person",
        "Movie",
        "producedId",
        [field("dataset")],
    )
    await session.link_create(
        "WROTE",
        "Person",
        "Movie",
        "wroteId",
        [field("dataset")],
    )
    await session.link_create(
        "REVIEWED",
        "Person",
        "Movie",
        "reviewedId",
        [field("summary"), field("rating", "int"), field("dataset")],
    )


async def get_or_create_person(session, people, name, born, dataset, kind="person"):
    if name not in people:
        people[name] = await session.node_create(
            "Person",
            {
                "name": name,
                "born": born,
                "kind": kind,
                "dataset": dataset,
            },
        )
    return people[name]


async def persist_movies(session: AsyncNeo4jSession, dataset: str):
    people = {}
    movies = {}
    counts = {
        "acted_in": 0,
        "directed": 0,
        "produced": 0,
        "wrote": 0,
        "reviewed": 0,
    }

    for movie in MOVIES:
        movie_node = await session.node_create(
            "Movie",
            {
                "title": movie["title"],
                "released": movie["released"],
                "tagline": movie["tagline"],
                "dataset": dataset,
            },
        )
        movies[movie["title"]] = movie_node

        for name, born, role in movie["cast"]:
            person = await get_or_create_person(session, people, name, born, dataset)
            await session.edge_create(
                "ACTED_IN",
                person["id"],
                movie_node["id"],
                {"role": role, "dataset": dataset},
            )
            counts["acted_in"] += 1

        for name, born in movie["directors"]:
            person = await get_or_create_person(session, people, name, born, dataset)
            await session.edge_create(
                "DIRECTED",
                person["id"],
                movie_node["id"],
                {"dataset": dataset},
            )
            counts["directed"] += 1

        for name, born in movie["producers"]:
            person = await get_or_create_person(session, people, name, born, dataset)
            await session.edge_create(
                "PRODUCED",
                person["id"],
                movie_node["id"],
                {"dataset": dataset},
            )
            counts["produced"] += 1

        for name, born in movie["writers"]:
            person = await get_or_create_person(session, people, name, born, dataset)
            await session.edge_create(
                "WROTE",
                person["id"],
                movie_node["id"],
                {"dataset": dataset},
            )
            counts["wrote"] += 1

    for reviewer, title, summary, rating in REVIEWS:
        person = await get_or_create_person(
            session,
            people,
            reviewer,
            0,
            dataset,
            kind="reviewer",
        )
        await session.edge_create(
            "REVIEWED",
            person["id"],
            movies[title]["id"],
            {"summary": summary, "rating": rating, "dataset": dataset},
        )
        counts["reviewed"] += 1

    return people, movies, counts


async def main() -> None:
    uri = require_env("NEO4J_URI")
    user = require_env("NEO4J_USER")
    password = require_env("NEO4J_PASSWORD")
    dataset = f"grm-python-movies-{time.time_ns()}"
    print(f"python movies dataset={dataset}")

    session = await AsyncNeo4jSession.connect(uri=uri, user=user, password=password)
    await define_schema(session)
    people, movies, counts = await persist_movies(session, dataset)

    reader = await AsyncNeo4jSession.connect(uri=uri, user=user, password=password)
    await define_schema(reader)
    persisted_movies = await reader.node_find("Movie", {"dataset": dataset})
    persisted_people = await reader.node_find("Person", {"dataset": dataset})
    assert len(persisted_movies) == len(movies), persisted_movies
    assert len(persisted_people) == len(people), persisted_people

    print("GRM Python Movies graph persisted:")
    print(f"  Movie nodes: {len(persisted_movies)}")
    print(f"  Person nodes: {len(persisted_people)}")
    print(f"  ACTED_IN relationships: {counts['acted_in']}")
    print(f"  DIRECTED relationships: {counts['directed']}")
    print(f"  PRODUCED relationships: {counts['produced']}")
    print(f"  WROTE relationships: {counts['wrote']}")
    print(f"  REVIEWED relationships: {counts['reviewed']}")
    print("note: ACTED_IN.role is a string placeholder for Neo4j's roles list.")
    print("inspect with:")
    print(
        "MATCH p=(:Person {dataset: '"
        + dataset
        + "'})-[:ACTED_IN|DIRECTED|PRODUCED|WROTE|REVIEWED]->(:Movie {dataset: '"
        + dataset
        + "'}) RETURN p LIMIT 100"
    )
    print("cleanup with:")
    print(f"MATCH (n {{dataset: '{dataset}'}}) DETACH DELETE n")


if __name__ == "__main__":
    asyncio.run(main())
