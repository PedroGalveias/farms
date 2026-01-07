/// Swiss canton capitals with their coordinates
/// Format: (City Name, "latitude,longitude")
pub const CANTON_CAPITALS: &[(&str, &str)] = &[
    ("Zürich", "47.3769,8.5417"),
    ("Bern", "46.9481,7.4474"),
    ("Lucerne", "47.0502,8.3093"),
    ("Altdorf", "46.8805,8.6444"),
    ("Schwyz", "47.0207,8.6532"),
    ("Sarnen", "46.8960,8.2461"),
    ("Stans", "46.9579,8.3659"),
    ("Glarus", "47.0404,9.0679"),
    ("Zug", "47.1724,8.5153"),
    ("Fribourg", "46.8063,7.1608"),
    ("Solothurn", "47.2084,7.5371"),
    ("Basel", "47.5596,7.5886"),
    ("Liestal", "47.4814,7.7343"),
    ("Schaffhausen", "47.6979,8.6344"),
    ("Herisau", "47.3859,9.2792"),
    ("Appenzell", "47.3316,9.4094"),
    ("St. Gallen", "47.4245,9.3767"),
    ("Chur", "46.8499,9.5331"),
    ("Aarau", "47.3925,8.0457"),
    ("Frauenfeld", "47.5536,8.8988"),
    ("Bellinzona", "46.1930,9.0208"),
    ("Lausanne", "46.5197,6.6323"),
    ("Sion", "46.2310,7.3603"),
    ("Neuchâtel", "46.9896,6.9294"),
    ("Geneva", "46.2044,6.1432"),
    ("Delémont", "47.3653,7.3453"),
];

/// Valid Swiss addresses representing different formats and language regions
pub const VALID_SWISS_ADDRESSES: &[&str] = &[
    // German-speaking region (Zürich, Bern, etc.)
    "Bahnhofstrasse 1, 8001 Zürich",
    "Hauptstrasse 23, 3000 Bern",
    "Dorfstrasse 12, 6340 Baar",
    "Bärenplatz 3, 3011 Bern",
    "Löwenstrasse 45, 8001 Zürich",
    "Rütistrasse 8, 8952 Schlieren",
    "Schützengasse 10, 8001 Zürich",
    // French-speaking region
    "Rue du Rhône 65, 1204 Genève",
    "Avenue de la Gare 5, 1003 Lausanne",
    "Rue de l'Hôtel-de-Ville 1, 1204 Genève",
    "Quai du Général-Guisan 28, 1204 Genève",
    "Place de la Gare 10, 1003 Lausanne",
    // Italian-speaking region
    "Via Nassa 5, 6900 Lugano",
    "Via della Stazione 8, 6900 Lugano",
    "Piazza della Riforma 1, 6900 Lugano",
    // Romansh-speaking region
    "Via Maistra 10, 7500 St. Moritz",
    // Addresses with building/apartment details
    "Wohnung 3B, Bahnhofstrasse 12, 8001 Zürich",
    "c/o Müller, Hauptstrasse 45, 3000 Bern",
    "Appartement 5, Rue du Rhône 10, 1204 Genève",
    "2. Stock, Dorfstrasse 8, 6340 Baar",
    // PO Box addresses
    "Postfach 1234, 3000 Bern",
    "Case postale 456, 1211 Genève",
    "Casella postale 789, 6900 Lugano",
    // Addresses with house number suffixes
    "Hauptstrasse 12a, 3000 Bern",
    "Dorfstrasse 5B, 8001 Zürich",
    "Via Nassa 10bis, 6900 Lugano",
    // Company addresses
    "Firma AG, Industriestrasse 1, 8050 Zürich",
    "Société SA, Rue du Commerce 5, 1204 Genève",
];
