import math

from beryl_tools.beryl3d import camera


def card(view: camera.View, name: str, x: float, y: float, turn: float) -> camera.Card:
    a, b, c, d = camera.card_corners(view, [x, y, 80.0, math.log(0.85), turn])
    return camera.Card("synthetic", "0:00", name, turn != 0.0, (a, b, c, d))


def test_the_fit_recovers_the_camera_that_projected_the_cards() -> None:
    truth = camera.View(pitch=30.0, fov=25.0)
    cards = [
        card(truth, f"card {i}", x, y, turn)
        for i, (x, y, turn) in enumerate(
            [
                (-300.0, -200.0, 0.0),
                (0.0, -200.0, 0.0),
                (300.0, -200.0, 0.0),
                (-300.0, 150.0, 0.0),
                (0.0, 150.0, 0.0),
                (300.0, 150.0, 0.0),
                (-150.0, -200.0, -0.14),
                (150.0, 150.0, -0.14),
            ]
        )
    ]
    fit = camera.solve(cards)
    assert abs(fit.view.pitch - 30.0) < 0.05
    assert abs(fit.view.fov - 25.0) < 0.05
    assert fit.rms < 1e-6


def test_the_measured_cards_give_the_camera_in_space_json() -> None:
    import json

    cards = camera.load(camera.fit_path())
    space = json.loads((camera.paths.REPO / "data/beryl3d/space.json").read_text())
    data = space["layout"]["desktop"]["camera"]
    fit = camera.solve(cards)
    assert round(fit.view.pitch, 2) == data["pitch_degrees"]
    assert round(fit.view.fov, 2) == data["fov_degrees"]
    assert fit.rms < 0.03
