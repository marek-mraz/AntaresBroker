ROBOT_LISTENER_API_VERSION = 2

import json
import os


class SimpleListener:

    ROBOT_LISTENER_API_VERSION = 2

    BASE_DIR = "/opt/robot-tests"
    REPORT_DIR = os.path.join(BASE_DIR, "listeners", "reports")
    REPORT_FILE = os.path.join(REPORT_DIR, "simple.json")

    def __init__(self):
        os.makedirs(self.REPORT_DIR, exist_ok=True)

        self.results = {}
        
        self.current_suite_path = None
        self.current_test_name = None

        self.suite_stack = []
        self.kw_stack = []
        self.actor_stack = []
        self.current_actor = "DefaultActor"
        self.in_call_api = False
        
        # Necessary to track if a keyword is from a SETUP, TEST or TEARDOWN
        self.keyword_scope = None

    # ------------------------------------------------------------
    # SUITES
    # ------------------------------------------------------------
    def start_suite(self, name, attrs):
        name = self._normalize(name)
        self.suite_stack.append(name)
        
        # If the suite name ends with '.robot', tests will actually take place.
        # If not, the direct next action will be to call a subfolder or a file.
        if attrs.get("source").endswith(".robot"):

            full_path = list(self.suite_stack)

            self.current_suite_path = "/".join(full_path)
            
            self.results[self.current_suite_path] = {
                "description": attrs.get("doc"),
                "actors" : [],
                "SETUP": [],
                "TESTS": {},
                "TEARDOWN": []
            }
            
            self.keyword_scope = "SETUP"

    def end_suite(self, name, attrs):
        if self.suite_stack:
            self.suite_stack.pop()
        self.keyword_scope = None

    # ------------------------------------------------------------
    # TESTS
    # ------------------------------------------------------------
    def start_test(self, name, attrs):
        self.current_test_name = name
        self.current_actor = "DefaultActor"
        self.keyword_scope = "TEST"

        self.results[self.current_suite_path]["TESTS"][name] = []

    def end_test(self, name, attrs):
        self.current_test_name = None
        self.current_actor = "DefaultActor"
        self.keyword_scope = "TEARDOWN"

    # ------------------------------------------------------------
    # KEYWORDS — start
    # ------------------------------------------------------------
    def start_keyword(self, name, attrs):

        kwname = self._splitter(attrs)
        kwname_lower = kwname.lower()
        
        if kwname_lower == "call api endpoint with invalid parameter":
            self.in_call_api = True
            self.actor_stack.append("DefaultActor")
            return
        
        tags = attrs.get("tags")
        
        if any("actor" in tag for tag in tags):
            actor = next((e for e in tags if "actor" in e))
            actor_formatted = ''.join(word.title() for word in actor.replace('actor_', '').split('-'))
            self.actor_stack.append(actor_formatted)
            # return

        if not (
            "check" in kwname_lower
            or kwname_lower in ("get", "post", "put", "patch", "delete")
            or "wait for" in kwname_lower
        ):
            self.kw_stack.append(None)
            return

        entry = {"keyword": kwname}

        self.kw_stack.append(entry)

    # ------------------------------------------------------------
    # KEYWORDS — end
    # ------------------------------------------------------------
    def end_keyword(self, name, attrs):

        kwname = self._splitter(attrs)
        kwname_lower = kwname.lower()
        
        if kwname_lower == "call api endpoint with invalid parameter":
            self.in_call_api = False
            entry = {"keyword": kwname}
            self.kw_stack.append(entry)                   

        if (
            not self.kw_stack
            or self.in_call_api
        ):
            return

        entry = self.kw_stack.pop()
        if entry is None:
            return

        suite = self.results[self.current_suite_path]

        if (
            kwname_lower in ("get", "post", "put", "patch", "delete")
            or "wait for" in kwname_lower
            or kwname_lower == "call api endpoint with invalid parameter"
        ):
            try:
                self.current_actor = self.actor_stack.pop()
            except:
                self.current_actor = "DefaultActor"
        
        if self.current_actor not in suite["actors"] :
            suite["actors"].append(self.current_actor)
            
        entry.update({
            "actor": self.current_actor
        })
        
        if self.keyword_scope == "TEST":
            suite["TESTS"][self.current_test_name].append(entry)

        else:
            suite[self.keyword_scope].append(entry)

    # ------------------------------------------------------------
    # UTILS
    # ------------------------------------------------------------
    
    def _normalize(self, name: str) -> str:
        return name.replace(" ", "_")
    
    def _splitter(self, attrs: str) -> str:
        kwname_raw = attrs.get("kwname", "").strip()

        if "." in kwname_raw:
            kwname = kwname_raw.split(".")[-1]
        else:
            kwname = kwname_raw

        return kwname

    # ------------------------------------------------------------
    # CLOSE
    # ------------------------------------------------------------
    def close(self):

        with open(self.REPORT_FILE, "w", encoding="utf-8") as f:
            json.dump(self.results, f, indent=2, ensure_ascii=False)

        print(f"[LISTENER] Report written to {self.REPORT_FILE}")