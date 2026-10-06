#!/usr/bin/env python3
"""Fill a running Aura Seeker API with realistic fake places in Auvergne-Rhône-Alpes cities.

The fake authors get their email marked as verified directly in the SQLite
database, since nobody can read the verification code sent to them.

Usage: seed_places.py COUNT [--url http://localhost:8080] [--database aura.db] [--seed N]
"""

import argparse
import datetime
import json
import math
import random
import sqlite3
import struct
import sys
import urllib.error
import urllib.request
import zlib

PASSWORD = "seed-password-1234"
AUTHORS = ["Camille", "Léa", "Hugo", "Inès", "Malo", "Nolwenn", "Théo", "Jade"]
BUILT_UP_SHARE = 0.7
EARTH_RADIUS = 6_371_000

CITIES = [
    ("Lyon", 45.75980, 4.83482, 520774, 48.09),
    ("Saint-Étienne", 45.44083, 4.39000, 172569, 80.16),
    ("Villeurbanne", 45.77917, 4.88194, 162207, 14.83),
    ("Grenoble", 45.18500, 5.72278, 156389, 18.61),
    ("Clermont-Ferrand", 45.77917, 3.08500, 147751, 43.14),
    ("Annecy", 45.90694, 6.12667, 131272, 16.04),
    ("Vénissieux", 45.70528, 4.88306, 66701, 15.37),
    ("Valence", 44.92694, 4.89500, 64288, 36.81),
    ("Chambéry", 45.57139, 5.91861, 60251, 21.28),
    ("Vaulx-en-Velin", 45.78667, 4.92500, 52448, 21.08),
    ("Saint-Priest", 45.69806, 4.94500, 49193, 29.67),
    ("Caluire-et-Cuire", 45.79806, 4.85111, 43479, 10.38),
    ("Bron", 45.73333, 4.90944, 42850, 10.28),
    ("Bourg-en-Bresse", 46.20528, 5.22639, 42065, 24.09),
    ("Montélimar", 44.55750, 4.74750, 40356, 46.57),
    ("Saint-Martin-d'Hères", 45.16556, 5.76889, 38022, 9.49),
    ("Oullins-Pierre-Bénite", 45.71611, 4.80639, 37928, 4.28),
    ("Thonon-les-Bains", 46.36917, 6.48444, 37689, 16.40),
    ("Annemasse", 46.19361, 6.23222, 37595, 4.92),
    ("Échirolles", 45.13917, 5.71972, 36708, 7.82),
    ("Meyzieu", 45.76556, 5.00000, 36437, 23.55),
    ("Villefranche-sur-Saône", 45.98889, 4.72389, 36224, 9.44),
    ("Saint-Chamond", 45.47278, 4.50750, 35586, 55.28),
    ("Roanne", 46.03472, 4.06972, 35364, 16.10),
    ("Montluçon", 46.34111, 2.60611, 33317, 20.79),
    ("Romans-sur-Isère", 45.04583, 5.05250, 33139, 33.49),
    ("Aix-les-Bains", 45.69417, 5.90944, 32175, 15.71),
    ("Vienne", 45.52528, 4.87583, 31555, 22.64),
    ("Rillieux-la-Pape", 45.82444, 4.89556, 31479, 14.67),
    ("Décines-Charpieu", 45.76889, 4.96389, 29905, 17.09),
    ("Bourgoin-Jallieu", 45.59083, 5.27778, 29816, 24.73),
    ("Aurillac", 44.92667, 2.44444, 26189, 29.28),
    ("Vichy", 46.13111, 3.42750, 25702, 5.87),
    ("Tassin-la-Demi-Lune", 45.76472, 4.76500, 22819, 7.95),
    ("Fontaine", 45.19250, 5.69639, 22471, 6.72),
    ("Oyonnax", 46.26083, 5.65472, 22378, 35.80),
    ("Sainte-Foy-lès-Lyon", 45.73667, 4.80417, 21893, 6.87),
    ("Voiron", 45.36778, 5.58972, 21604, 22.06),
    ("Saint-Genis-Laval", 45.69639, 4.79472, 21329, 12.96),
    ("Givors", 45.59167, 4.76972, 20943, 17.57),
    ("Cournon-d'Auvergne", 45.74083, 3.19639, 20020, 18.89),
]
POPULATIONS = [population for _, _, _, population, _ in CITIES]

SPOTS = [
    "jardin public",
    "parc municipal",
    "place du marché",
    "centre-ville",
    "quartier de la gare",
    "bords de rivière",
    "vieille ville",
    "hauteurs",
    "esplanade",
    "square de l'hôtel de ville",
]

KINDS = [
    {
        "titles": ["Banc tranquille", "Banc à l'ombre", "Banc avec vue", "Vieux banc en bois"],
        "tags": ["banc", "calme", "ombre", "lecture"],
        "descriptions": [
            "Parfait pour lire une heure sans être dérangé.",
            "À l'abri du vent, souvent libre en semaine.",
            "Le dossier est un peu bancal mais la vue rattrape tout.",
        ],
    },
    {
        "titles": ["Point de vue", "Panorama", "Belvédère caché", "Spot coucher de soleil"],
        "tags": ["panorama", "coucher de soleil", "photo", "calme"],
        "descriptions": [
            "Vue dégagée sur toute la ville, à voir en fin de journée.",
            "Peu de monde le matin, lumière superbe en automne.",
            "Il faut grimper un peu mais ça vaut le détour.",
        ],
    },
    {
        "titles": ["Fontaine d'eau potable", "Point d'eau", "Fontaine fraîche"],
        "tags": ["eau potable", "fontaine", "vélo", "running"],
        "descriptions": [
            "Eau fraîche toute l'année, pratique pour remplir sa gourde.",
            "Coupée en hiver, fonctionne d'avril à octobre.",
        ],
    },
    {
        "titles": ["Fresque murale", "Graffiti géant", "Collage éphémère", "Mur d'expression"],
        "tags": ["street art", "photo", "insolite"],
        "descriptions": [
            "Renouvelée régulièrement, jamais la même d'un mois à l'autre.",
            "Cachée derrière le porche, on passe facilement à côté.",
        ],
    },
    {
        "titles": ["Coin pique-nique", "Pelouse au soleil", "Table de pique-nique", "Clairière"],
        "tags": ["pique-nique", "ombre", "famille", "calme"],
        "descriptions": [
            "Grande table en bois, poubelles à proximité.",
            "Herbe bien entretenue, idéale pour un déjeuner au soleil.",
            "Assez grand pour un groupe, penser à venir tôt le week-end.",
        ],
    },
    {
        "titles": ["Arbre remarquable", "Vieux chêne", "Grand cèdre", "Saule au bord de l'eau"],
        "tags": ["nature", "ombre", "insolite", "photo"],
        "descriptions": [
            "Plus de deux siècles d'après le panneau, tronc impressionnant.",
            "L'ombre la plus fraîche du quartier en plein été.",
        ],
    },
    {
        "titles": ["Boîte à livres", "Bibliothèque de rue", "Cabane à livres"],
        "tags": ["lecture", "boîte à livres", "insolite"],
        "descriptions": [
            "Bien fournie, surtout en romans policiers et en BD.",
            "On y trouve parfois des livres pour enfants en très bon état.",
        ],
    },
]


class Api:
    def __init__(self, url, database):
        self.url = url.rstrip("/")
        self.database = database

    def call(self, method, path, body=None, token=None, content_type="application/json"):
        data = json.dumps(body).encode() if content_type == "application/json" and body is not None else body
        headers = {"content-type": content_type} if data is not None else {}
        if token:
            headers["authorization"] = f"Bearer {token}"
        request = urllib.request.Request(self.url + path, data=data, headers=headers, method=method)
        with urllib.request.urlopen(request, timeout=30) as response:
            content = response.read()
            return json.loads(content) if content else None

    def session(self, email):
        credentials = {"email": email, "password": PASSWORD}
        try:
            tokens = self.call("POST", "/auth/password/login", credentials)
        except urllib.error.HTTPError as error:
            if error.code != 401:
                raise
            self.call("POST", "/auth/password/signup", credentials)
            self.mark_verified(email)
            tokens = self.call("POST", "/auth/password/login", credentials)
        return tokens["access_token"], tokens["user"]["id"]

    def mark_verified(self, email):
        connection = sqlite3.connect(f"file:{self.database}?mode=rw", uri=True, timeout=10)
        with connection:
            connection.execute(
                "UPDATE users SET email_verified_at = ? WHERE email = ? AND email_verified_at IS NULL",
                (verified_at(), email),
            )
        connection.close()


def verified_at():
    return datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")


def authors(api):
    sessions = []
    for name in AUTHORS:
        token, user_id = api.session(f"{name.lower()}@seed.example.com")
        api.call("PUT", f"/users/{user_id}", {"display_name": name}, token)
        sessions.append(token)
    return sessions


def png(red, green, blue, size=64):
    def chunk(kind, data):
        payload = kind + data
        return struct.pack(">I", len(data)) + payload + struct.pack(">I", zlib.crc32(payload))

    row = b"\x00" + bytes((red, green, blue)) * size
    header = struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(row * size)) + chunk(b"IEND", b"")


def scattered(latitude, longitude, area_km2):
    city_radius = math.sqrt(area_km2 / math.pi) * 1000
    distance = city_radius * BUILT_UP_SHARE * math.sqrt(random.random())
    bearing = random.uniform(0, 2 * math.pi)
    north = distance * math.cos(bearing) / EARTH_RADIUS
    east = distance * math.sin(bearing) / (EARTH_RADIUS * math.cos(math.radians(latitude)))
    return round(latitude + math.degrees(north), 6), round(longitude + math.degrees(east), 6)


def fake_place(image):
    city, latitude, longitude, _, area_km2 = random.choices(CITIES, weights=POPULATIONS)[0]
    kind = random.choice(KINDS)
    latitude, longitude = scattered(latitude, longitude, area_km2)
    place = {
        "title": f"{random.choice(kind['titles'])} — {random.choice(SPOTS)}, {city}",
        "latitude": latitude,
        "longitude": longitude,
        "image": image,
        "tags": random.sample(kind["tags"], random.randint(1, 3)) + [city.lower()],
    }
    if random.random() < 0.8:
        place["description"] = random.choice(kind["descriptions"])
    return place


def seed(api, count):
    sessions = authors(api)
    for number in range(1, count + 1):
        token = random.choice(sessions)
        colour = [random.randint(40, 215) for _ in range(3)]
        image = api.call("POST", "/files", png(*colour), token, content_type="image/png")["id"]
        place = api.call("POST", "/places", fake_place(image), token)
        print(f"[{number}/{count}] {place['title']} ({place['latitude']}, {place['longitude']}) {place['tags']}")


def main():
    parser = argparse.ArgumentParser(description="Fill a running Aura Seeker API with realistic fake places.")
    parser.add_argument("count", type=int, help="number of places to create")
    parser.add_argument("--url", default="http://localhost:8080", help="base URL of the API")
    parser.add_argument("--database", default="aura.db", help="SQLite file of that API, to verify the fake authors")
    parser.add_argument("--seed", type=int, help="random seed, for reproducible places")
    arguments = parser.parse_args()
    if arguments.count < 1:
        parser.error("count must be at least 1")
    random.seed(arguments.seed)
    try:
        seed(Api(arguments.url, arguments.database), arguments.count)
    except urllib.error.HTTPError as error:
        sys.exit(f"the API answered {error.code} on {error.url}: {error.read().decode(errors='replace')}")
    except sqlite3.Error as error:
        sys.exit(f"cannot mark the fake authors as verified in {arguments.database}: {error}")
    except urllib.error.URLError as error:
        sys.exit(f"cannot reach the API at {arguments.url}: {error.reason}")


if __name__ == "__main__":
    main()
