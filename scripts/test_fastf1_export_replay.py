import contextlib
import inspect
import io
import json
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from typing import Any
from unittest.mock import patch

import pandas as pd
import urllib3

sys.path.insert(0, str(Path(__file__).resolve().parent))
import fastf1_export_replay as export


class FastF1IntervalExportTests(unittest.TestCase):
    def test_export_intervals_formats_leader_gap_and_interval(self):
        telemetry = [
            sample(1, 10.0, 2, 0.500, lap_duration=100.0),
            sample(16, 10.0, 2, 0.450, lap_duration=100.0),
            sample(44, 10.0, 2, 0.425, lap_duration=100.0),
        ]

        intervals = export.export_intervals(telemetry)

        self.assertEqual(
            intervals,
            [
                {"t": 10.0, "driver_number": 1, "gap_to_leader": None, "interval": None},
                {"t": 10.0, "driver_number": 16, "gap_to_leader": "+5.000", "interval": "+5.000"},
                {"t": 10.0, "driver_number": 44, "gap_to_leader": "+7.500", "interval": "+2.500"},
            ],
        )

    def test_export_intervals_labels_lapped_cars(self):
        telemetry = [
            sample(1, 12.0, 4, 0.100, lap_duration=90.0),
            sample(4, 12.0, 2, 0.950, lap_duration=90.0),
            sample(31, 12.0, 2, 0.900, lap_duration=90.0),
        ]

        intervals = export.export_intervals(telemetry)

        self.assertEqual(intervals[1]["gap_to_leader"], "+1 LAP")
        self.assertEqual(intervals[1]["interval"], "+1 LAP")
        self.assertEqual(intervals[2]["gap_to_leader"], "+1 LAP")
        self.assertEqual(intervals[2]["interval"], "+4.500")

    def test_export_intervals_ignores_missing_relative_distance(self):
        telemetry = [
            sample(1, 10.0, 2, 0.500, lap_duration=100.0),
            {"driver_number": 16, "t": 10.0, "lap_number": 2, "relative_distance": None},
        ]

        intervals = export.export_intervals(telemetry)

        self.assertEqual(len(intervals), 1)
        self.assertEqual(intervals[0]["driver_number"], 1)

    def test_positions_and_intervals_use_same_frame_cadence(self):
        telemetry = [
            sample(1, 10.12, 2, 0.500),
            sample(16, 10.13, 2, 0.450),
            sample(1, 10.64, 2, 0.510),
            sample(16, 10.65, 2, 0.460),
        ]

        position_times = {row["t"] for row in export.export_positions(telemetry)}
        interval_times = {row["t"] for row in export.export_intervals(telemetry)}

        self.assertEqual(position_times, interval_times)
        self.assertEqual(position_times, {10.0, 10.5})


class FastF1ResultStatusTests(unittest.TestCase):
    def test_classification_status_maps_finished_and_lapped_to_not_dnf(self):
        self.assertFalse(export.is_dnf_status("Finished"))
        self.assertFalse(export.is_dnf_status("+1 Lap"))
        self.assertFalse(export.is_dnf_status("12"))

    def test_classification_status_maps_retirements_to_dnf(self):
        self.assertTrue(export.is_dnf_status("Accident"))
        self.assertTrue(export.is_dnf_status("Engine"))
        self.assertTrue(export.is_dnf_status("Retired"))

    def test_dns_and_dsq_are_distinct_from_dnf(self):
        self.assertTrue(export.is_dns_status("Did not start"))
        self.assertTrue(export.is_dsq_status("Disqualified"))
        self.assertFalse(export.is_dnf_status("Did not start"))
        self.assertFalse(export.is_dnf_status("Disqualified"))


class FastF1ScheduleResolverTests(unittest.TestCase):
    def test_resolves_saudi_arabian_metadata_against_saudi_arabia_schedule(self):
        schedule = pd.DataFrame(
            [
                {
                    "RoundNumber": 2,
                    "EventName": "Saudi Arabian Grand Prix",
                    "OfficialEventName": "FORMULA 1 STC SAUDI ARABIAN GRAND PRIX 2024",
                    "Country": "Saudi Arabia",
                    "Location": "Jeddah",
                    "EventDate": pd.Timestamp("2024-03-09T17:00:00Z"),
                }
            ]
        )

        with patch.object(export, "fastf1", FakeFastF1(schedule)):
            round_number, metadata = export.resolve_round(
                2024,
                None,
                "Saudi Arabian Grand Prix",
                "Saudi Arabia",
                "Jeddah",
                "2024-03-09T17:00:00Z",
            )

        self.assertEqual(round_number, 2)
        self.assertEqual(metadata["match_method"], "fastf1_schedule_match")

    def test_uses_session_date_when_text_metadata_is_not_confident(self):
        schedule = pd.DataFrame(
            [
                {
                    "RoundNumber": 5,
                    "EventName": "Known Event",
                    "OfficialEventName": "Known Event",
                    "Country": "Known",
                    "Location": "Known",
                    "EventDate": pd.Timestamp("2024-05-05T12:00:00Z"),
                }
            ]
        )

        with patch.object(export, "fastf1", FakeFastF1(schedule)):
            round_number, metadata = export.resolve_round(
                2024,
                None,
                "Completely Unrelated",
                "Elsewhere",
                "Nowhere",
                "2024-05-05T14:00:00Z",
            )

        self.assertEqual(round_number, 5)
        self.assertEqual(metadata["match_method"], "fastf1_schedule_date_match")
        self.assertTrue(metadata["warnings"])


class ExportGuardTests(unittest.TestCase):
    def test_empty_laps_fail_with_a_sign_in_hint_when_not_authenticated(self) -> None:
        with self.assertRaises(export.ExportError) as ctx:
            export.require_lap_data(FakeSession(pd.DataFrame()), authenticated=False)
        self.assertIn("F1 TV subscription", str(ctx.exception))
        self.assertIn("F1_USERNAME", str(ctx.exception))

    def test_empty_laps_fail_with_an_archive_hint_when_authenticated(self) -> None:
        with self.assertRaises(export.ExportError) as ctx:
            export.require_lap_data(FakeSession(pd.DataFrame()), authenticated=True)
        self.assertIn("archive", str(ctx.exception))
        self.assertNotIn("subscription", str(ctx.exception))

    def test_unloaded_laps_fail_instead_of_exporting_an_empty_bundle(self) -> None:
        with self.assertRaises(export.ExportError):
            export.require_lap_data(FakeSession(None), authenticated=True)

    def test_loaded_laps_pass(self) -> None:
        export.require_lap_data(FakeSession(pd.DataFrame([{"LapNumber": 1}])), authenticated=False)

    def test_main_reports_export_errors_as_one_stderr_line(self) -> None:
        stderr = io.StringIO()
        with patch.object(export, "export_bundle", side_effect=export.ExportError("boom")):
            with contextlib.redirect_stderr(stderr):
                code = export.main(
                    ["--year", "2024", "--session", "R", "--session-key", "1", "--output", "x.json"]
                )
        self.assertEqual(code, 1)
        self.assertEqual(stderr.getvalue(), "error: boom\n")

    def test_main_auth_check_prints_json_and_skips_export_arguments(self) -> None:
        stdout = io.StringIO()
        with patch.object(export, "auth_check", return_value={"result": "missing", "message": "m"}):
            with contextlib.redirect_stdout(stdout):
                code = export.main(["--auth-check"])
        self.assertEqual(code, 1)
        self.assertEqual(json.loads(stdout.getvalue()), {"result": "missing", "message": "m"})

    def test_main_still_requires_export_arguments(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                export.main(["--year", "2024"])


@unittest.skipIf(export.fastf1 is None, "fastf1 is not installed")
class ArchiveRejectionTests(unittest.TestCase):
    PATH = "/static/2024/x/"
    PRIMARY = "https://livetiming.formula1.com/"
    MIRROR = "https://livetiming-mirror.fastf1.dev/"

    def setUp(self) -> None:
        self.cache = export.fastf1.Cache
        self.hosts = export.archive_hosts(export.fastf1._api)
        cache_dir = tempfile.TemporaryDirectory(prefix="interval-cache-")
        self.addCleanup(cache_dir.cleanup)
        self.cache_dir = cache_dir.name
        self.addCleanup(self.restore_cache, self.cache._CACHE_DIR, self.cache._requests_session_cached)

    def restore_cache(self, cache_dir: str | None, session: Any) -> None:
        if self.cache._requests_session_cached is not None:
            self.cache._requests_session_cached.close()
        self.cache._CACHE_DIR = cache_dir
        self.cache._requests_session_cached = session

    def configure(self, statuses: dict[str, int]) -> None:
        self.cache.configure(cache_dir=self.cache_dir)
        mount(self.cache, statuses)

    def fetch(self) -> Any:
        return export.fastf1._api.fetch_page(self.PATH, "timing_data")

    def test_primary_rejection_followed_by_mirror_success_is_not_a_rejection(self) -> None:
        self.configure({self.PRIMARY: 403, self.MIRROR: 200})
        with export.watch_livetiming_auth(self.cache, self.hosts) as watch:
            page = self.fetch()
        self.assertEqual(page, [])
        self.assertEqual(watch.rejections, [])
        watch.raise_if_rejected(authenticated=False)

    def test_unrelated_host_rejections_are_ignored(self) -> None:
        self.configure({"https://api.jolpi.ca/": 403})
        with export.watch_livetiming_auth(self.cache, self.hosts) as watch:
            response = self.cache.requests_get("https://api.jolpi.ca/ergast/f1/2024.json")
        self.assertEqual(response.status_code, 403)
        self.assertEqual(watch.rejections, [])

    def test_final_archive_rejections_are_recorded(self) -> None:
        self.configure({})
        for status in (401, 403):
            with self.subTest(status=status):
                mount(self.cache, {self.PRIMARY: status, self.MIRROR: status})
                with export.watch_livetiming_auth(self.cache, self.hosts) as watch:
                    self.assertIsNone(self.fetch())
                self.assertEqual(watch.rejections, [(status, f"{self.MIRROR}static/2024/x/TimingData.jsonStream")])
        with self.assertRaises(export.ExportError) as ctx:
            watch.raise_if_rejected(authenticated=False)
        self.assertIn("HTTP 403", str(ctx.exception))
        self.assertIn("F1 TV subscription", str(ctx.exception))
        with self.assertRaises(export.ExportError) as ctx:
            watch.raise_if_rejected(authenticated=True)
        self.assertIn("subscription is active", str(ctx.exception))

    def test_requests_get_is_restored_after_success_and_error(self) -> None:
        original = inspect.getattr_static(self.cache, "requests_get")
        with export.watch_livetiming_auth(self.cache, self.hosts):
            self.assertIsNot(inspect.getattr_static(self.cache, "requests_get"), original)
        self.assertIs(inspect.getattr_static(self.cache, "requests_get"), original)
        with self.assertRaises(RuntimeError), export.watch_livetiming_auth(self.cache, self.hosts):
            raise RuntimeError("load failed")
        self.assertIs(inspect.getattr_static(self.cache, "requests_get"), original)

    def test_load_session_fails_when_telemetry_is_rejected_despite_cached_laps(self) -> None:
        fake = FakeFastF1Loader(self.cache, {self.PRIMARY: 403, self.MIRROR: 403})
        with patch.object(export, "fastf1", fake):
            with self.assertRaises(export.ExportError) as ctx:
                export.load_session(2024, 1, "R", self.cache_dir, True)
        self.assertIn("HTTP 403", str(ctx.exception))
        self.assertTrue(fake.session.loaded)

    def test_load_session_succeeds_when_the_mirror_serves_what_the_primary_rejects(self) -> None:
        fake = FakeFastF1Loader(self.cache, {self.PRIMARY: 403, self.MIRROR: 200})
        with patch.object(export, "fastf1", fake):
            session = export.load_session(2024, 1, "R", self.cache_dir, False)
        self.assertIs(session, fake.session)
        self.assertEqual(session.page, [])


@unittest.skipIf(export.fastf1 is None, "fastf1 is not installed")
class F1AccountAuthTests(unittest.TestCase):
    def setUp(self) -> None:
        self.account = export.f1account()
        export.configure_auth()
        self.addCleanup(self.account.token_manager.clear)

    def credentials(self, username: str = "driver@example.com", password: str = "hunter2") -> Any:
        return patch.dict(os.environ, {"F1_USERNAME": username, "F1_PASSWORD": password})

    def no_credentials(self) -> Any:
        env = {key: value for key, value in os.environ.items() if key not in ("F1_USERNAME", "F1_PASSWORD")}
        return patch.dict(os.environ, env, clear=True)

    def auth_error(self, message: str, cause: BaseException | None = None) -> Any:
        error = export.fastf1.exceptions.F1AuthError(message)

        def raise_error(_credentials: Any) -> None:
            raise error from cause

        return patch.object(self.account, "login", side_effect=raise_error)

    def token(self, subject: str) -> Any:
        claims = {"exp": time.time() + 3600, "sub": subject, "SubscriptionStatus": "active"}
        return self.account.SubscriptionToken.from_jwt(encode_jwt(claims))

    def test_tokens_live_only_in_memory_for_one_run(self) -> None:
        self.assertIsInstance(self.account.token_manager._store, export.SessionTokenStore)
        self.assertEqual(self.account.get_auth_method(), "password")
        with self.credentials(), patch.object(self.account, "login", return_value=self.token("a")) as login:
            first = self.account.get_token()
            second = self.account.get_token()
            export.configure_auth()
            third = self.account.get_token()

        self.assertEqual(first.value, second.value)
        self.assertEqual(third.value, first.value)
        self.assertEqual(login.call_count, 2)

    def test_switching_accounts_signs_in_as_the_new_account(self) -> None:
        tokens = {"a@example.test": self.token("a"), "b@example.test": self.token("b")}

        def login(credentials: Any) -> Any:
            return tokens[credentials.username]

        with patch.object(self.account, "login", side_effect=login) as login_mock:
            with self.credentials("a@example.test", "pw-a"):
                export.configure_auth()
                self.assertEqual(self.account.get_token().value, tokens["a@example.test"].value)
            with self.credentials("b@example.test", "pw-b"):
                export.configure_auth()
                self.assertEqual(self.account.get_token().value, tokens["b@example.test"].value)

        self.assertEqual([call.args[0].username for call in login_mock.call_args_list], ["a@example.test", "b@example.test"])

    def test_changed_password_signs_in_again_with_the_new_password(self) -> None:
        with patch.object(self.account, "login", return_value=self.token("a")) as login:
            with self.credentials("a@example.test", "old-password"):
                export.configure_auth()
                self.account.get_token()
            with self.credentials("a@example.test", "new-password"):
                export.configure_auth()
                self.account.get_token()

        self.assertEqual([call.args[0].password for call in login.call_args_list], ["old-password", "new-password"])

    def test_failed_sign_in_leaves_no_token_behind(self) -> None:
        with self.credentials(), self.auth_error("Sign-in was rejected. Check username and password."):
            self.assertEqual(export.auth_check()["result"], "unauthorized")
            with self.assertRaises(export.fastf1.exceptions.F1AuthError):
                self.account.get_token()
        self.assertIsNone(self.account.token_manager._store.load())

    def test_auth_check_reports_missing_credentials_without_prompting(self) -> None:
        with self.no_credentials(), patch.object(sys, "stdin", FakeTty()):
            with patch("builtins.input", side_effect=AssertionError("prompted")):
                with patch("getpass.getpass", side_effect=AssertionError("prompted")):
                    result = export.auth_check()
        self.assertEqual(result["result"], "missing")
        self.assertIn("F1_USERNAME", result["message"])

    def test_auth_check_signs_in_with_the_configured_credentials(self) -> None:
        token = self.account.SubscriptionToken.from_jwt(
            encode_jwt({"exp": time.time() + 3600, "SubscriptionStatus": "active", "SubscribedProduct": "F1 TV Premium"})
        )
        with self.credentials(), patch.object(self.account, "login", return_value=token) as login:
            result = export.auth_check()

        self.assertEqual(result["result"], "ok")
        self.assertIn("active", result["message"])
        self.assertIn("F1 TV Premium", result["message"])
        self.assertEqual(login.call_args.args[0].username, "driver@example.com")
        self.assertEqual(login.call_args.args[0].password, "hunter2")

    def test_auth_check_classifies_sign_in_failures(self) -> None:
        connection_error = None
        if export.requests is not None:
            connection_error = export.requests.ConnectionError("no route")
        cases = [
            ("Sign-in was rejected. Check username and password.", None, "unauthorized"),
            ("Access token contains no subscription token. An active F1TV subscription is required.", None, "no_subscription"),
            ("Resuming the sign-in did not redirect (status 500).", None, "invalid"),
            ("Sign-in failed: no route", connection_error, "unreachable"),
        ]
        for message, cause, expected in cases:
            with self.subTest(expected=expected), self.credentials(), self.auth_error(message, cause):
                result = export.auth_check()
            self.assertEqual(result["result"], expected)
            self.assertEqual(result["message"], message)

    def test_export_preflight_fails_before_the_session_is_loaded(self) -> None:
        with self.credentials(), self.auth_error("Sign-in was rejected. Check username and password."):
            with patch.object(export, "load_session", side_effect=AssertionError("loaded")):
                with self.assertRaises(export.ExportError) as ctx:
                    export.export_bundle(2024, 1, "R", 9472, "unused", None, None, None, None, "provided_round")
        self.assertIn("sign-in failed", str(ctx.exception))
        self.assertIn("username and password", str(ctx.exception))

    def test_export_without_credentials_does_not_sign_in_and_fails_on_empty_data(self) -> None:
        with self.no_credentials(), patch.object(self.account, "login", side_effect=AssertionError("signed in")):
            with patch.object(export, "load_session", return_value=FakeSession(pd.DataFrame())):
                with self.assertRaises(export.ExportError) as ctx:
                    export.export_bundle(2024, 1, "R", 9472, "unused", None, None, None, None, "provided_round")
        self.assertIn("F1 TV subscription", str(ctx.exception))


def encode_jwt(claims: dict[str, Any]) -> str:
    import jwt

    return jwt.encode(claims, "test-secret-that-is-long-enough-for-hs256", algorithm="HS256")


class FakeTty(io.StringIO):
    def isatty(self) -> bool:
        return True


class FakeSession:
    def __init__(self, laps: pd.DataFrame | None) -> None:
        self._laps = laps

    @property
    def laps(self) -> pd.DataFrame:
        if self._laps is None:
            raise RuntimeError("not loaded")
        return self._laps


class StatusAdapter(export.requests.adapters.HTTPAdapter if export.requests else object):
    def __init__(self, status: int) -> None:
        super().__init__()
        self.status = status

    def send(self, request: Any, **kwargs: Any) -> Any:
        raw = urllib3.response.HTTPResponse(
            body=io.BytesIO(b""), status=self.status, headers={}, preload_content=False, request_url=request.url
        )
        return self.build_response(request, raw)


def mount(cache: Any, statuses: dict[str, int]) -> None:
    for prefix, status in statuses.items():
        cache._requests_session_cached.mount(prefix, StatusAdapter(status))


class LoadingSession:
    def __init__(self, cache: Any, api: Any, statuses: dict[str, int]) -> None:
        self.cache = cache
        self.api = api
        self.statuses = statuses
        self.loaded = False
        self.page: Any = None
        self.laps = pd.DataFrame([{"LapNumber": 1}])

    def load(self, **_kwargs: Any) -> None:
        mount(self.cache, self.statuses)
        self.page = self.api.fetch_page(ArchiveRejectionTests.PATH, "timing_data")
        self.loaded = True


class FakeFastF1Loader:
    def __init__(self, cache: Any, statuses: dict[str, int]) -> None:
        self.Cache = cache
        self._api = export.fastf1._api
        self.session = LoadingSession(cache, self._api, statuses)

    def get_session(self, year: int, round_number: int, session_code: str) -> LoadingSession:
        return self.session


def sample(driver_number, t, lap_number, relative_distance, lap_duration=90.0):
    return {
        "driver_number": driver_number,
        "t": t,
        "lap_number": lap_number,
        "lap_duration": lap_duration,
        "relative_distance": relative_distance,
    }


class FakeFastF1:
    def __init__(self, schedule):
        self.schedule = schedule

    def get_event_schedule(self, year):
        return self.schedule


if __name__ == "__main__":
    unittest.main()
