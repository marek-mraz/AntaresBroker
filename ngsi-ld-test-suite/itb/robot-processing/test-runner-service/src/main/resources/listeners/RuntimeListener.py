ROBOT_LISTENER_API_VERSION = 2

import ast
import json
import re
import os

from robot.libraries.BuiltIn import BuiltIn

class RuntimeListener:

    ROBOT_LISTENER_API_VERSION = 2

    BASE_DIR = "/opt/robot-tests"
    REPORT_DIR = os.path.join(BASE_DIR, "listeners", "reports")
    REPORT_FILE = os.path.join(REPORT_DIR, "runtime.json")

    def __init__(self):
        print("Listener.__init__ called")
        os.makedirs(self.REPORT_DIR, exist_ok=True)

        self._var_pattern = re.compile(r"\$\{([^}]+)\}")

        # Stack to ensure the keywords will not overlap
        self.kw_stack = []
        
        self.results_stack = []

        self.results = {
            "SETUP": [],
            "TEST": [],
            "TEARDOWN": []
        }
        
        # Necessary to track if a keyword is from a SETUP, TEST or TEARDOWN
        self.kw_scope = "SETUP"
        
        self.logs = []
        
        self.kw_args_stack = []
        
        self.subkw_stack = []

        # Will save the content of the JSON input called in the robot tests
        self._file_content_cache = {}

    # ------------------------------------------------------------
    # TESTS
    # ------------------------------------------------------------
    def start_test(self, name, attrs):
        self.kw_scope = "TEST"

    def end_test(self, name, attrs):
        self.kw_scope = "TEARDOWN"

    # ------------------------------------------------------------
    # KEYWORDS — start
    # ------------------------------------------------------------
    def start_keyword(self, name, attrs):
        # This steps need to be done at the start of the keyword to be stacked in order
        
        kwname = attrs.get("kwname", "")
        kwname_normalized = kwname.casefold()
            
        # the "load json from file" keyword will be catch 
        # to retrieve the path of the inputs used in the "check response body" keywords
        if (
            "check" in kwname_normalized
            or "should" in kwname_normalized
            or kwname_normalized in ("get", "post", "put", "patch", "delete", "load json from file")
        ):
            entry = {"name": kwname,
                     "args" : {}}

        else:
            entry = None
            
        if ("check" in kwname_normalized):
            self.subkw_stack = []

        self.kw_stack.append(entry)

    # ------------------------------------------------------------
    # KEYWORDS — end
    # ------------------------------------------------------------

    def end_keyword(self, name, attrs):
        if not self.kw_stack:
            return

        entry = self.kw_stack.pop()
        
        if entry is None:
            return
        
        kwname_normalized = entry["name"].casefold()
        kw_args = attrs.get("args")
                
        if kwname_normalized == "load json from file":
            
            builtin = BuiltIn()
            input_path = kw_args[0]
            
            exe_dir = builtin.get_variable_value("${EXECDIR}", default="")
            path_with_vars = input_path.replace("${EXECDIR}", exe_dir)
            
            var_match = self._var_pattern.search(path_with_vars)
            
            if var_match:
                name = var_match.group(1)
                var_name = "${" + name + "}"
                var_value = builtin.get_variable_value(var_name, default=None)
                
                if var_value is not None:
                    full_path = path_with_vars.replace(var_name, var_value)
                    
                    if os.path.isfile(full_path):
                        try:
                            with open(full_path, "r", encoding="utf-8") as f:
                                file_content = f.read()
                            self._file_content_cache[var_value] = file_content
                            
                        except Exception as e:
                            print(f"[LISTENER][ERROR] Cannot loading JSON {full_path}: {e}")
            # We don't keep the keyword, the only relevant info is the file_content
            return
        
        if "should" in kwname_normalized:
            names = [self._extract_var_name(a) for a in (kw_args or [])]
            labelled_args = [self._label_arg(a) for a in (kw_args or [])]
            prettiffied_args = [self._prettier(a) for a in (labelled_args or [])]
            entry["args"].update({
                "names": names,
                "values": prettiffied_args
            })
            self.subkw_stack.append(entry)
            # We don't keep it as a keyword in the listener report
            return

        entry.update({
            "status": attrs.get("status")
        })

        self.results_stack.append(
            {"kw_scope" : self.kw_scope,
             "kw_entry" : entry}
            )
        
        # Must happen exactly now :
        # After the keyword execution : to retrieve the real value of the variables.
        # Before the end of the suite : because it calls a BuiltIn()
        if "check" in kwname_normalized:
            self.kw_args_stack.append(self.subkw_stack)       

    def log_message(self, message):
        self.logs.append(message.get("message", ""))
                            
    def format_result(self):
        while self.results_stack:
            pop = self.results_stack.pop(0)
            kw_entry = pop["kw_entry"]
            kw_scope = pop["kw_scope"]
            kwname_normalized = kw_entry["name"].casefold()
            
            if kwname_normalized in ("get", "post", "put", "patch", "delete"):
                while self.logs:
                    message = self.logs.pop(0)
                    if message == "Request ->":                        
                        raw_response = self.logs.pop(0)
                        kw_entry["args"]["request"] = self._response_parser(raw_response)
                        break
                while self.logs:
                    message = self.logs.pop(0)
                    if message == "Response ->":
                        raw_response = self.logs.pop(0)
                        kw_entry["args"]["response"] = self._response_parser(raw_response)
                        break
                self.results[kw_scope].append(kw_entry)
                continue
                    
            check_args = self.kw_args_stack.pop(0)
            while check_args:
                subkw = check_args.pop(0)
                kw_entry["args"][subkw["name"]] = {}
                if kwname_normalized == "check response status code":
                    length = 2
                else:
                    length = len(subkw["args"]["names"])
                for i in range(length):
                    arg_name = subkw["args"]["names"][i]
                    arg_value = subkw["args"]["values"][i]
                    kw_entry["args"][subkw["name"]][arg_name] = arg_value                
                    
            self.results[kw_scope].append(kw_entry)
        return self.results
    
    def _prettier(self, arg):
        try:
            data = ast.literal_eval(arg)
            pretty = json.dumps(data, indent=4)
            return pretty
        except Exception:
            return arg

    # By default the arguments displayed are the name of the robot variable and not the value of it
    # This function is called to replaced such a name by the actual value of a test variable.
    def _label_arg(self, arg):
        """
        - if arg is dict -> return pretty JSON.
        - if arg is a string like '${var} = <json|list>' -> keep it as is.
        - else : for each ${var} found, return '${var} = <value>' separated by ',' if several.
        """
        
        if isinstance(arg, (dict, list)):
            try:
                return json.dumps(arg, ensure_ascii=False)
            except Exception:
                return str(arg)

        if not isinstance(arg, str):
            return str(arg)

        m = re.match(r'^\s*(\$\{[^}]+\})\s*=\s*(\{.*\}|\[.*\])\s*$', arg, flags=re.DOTALL)
        if m:
            rhs = m.group(2)
            try:
                parsed = json.loads(rhs)
                pretty = json.dumps(parsed, ensure_ascii=False)
                return f"{pretty}"
            except Exception:
                return arg

        matches = self._var_pattern.findall(arg)
        if not matches:
            return arg

        builtin = BuiltIn()
        parts = []
        for var_name in matches:
            token = f"${{{var_name}}}"
            try:
                val = builtin.get_variable_value(token, default=None)
            except Exception:
                val = None
            parts.append(f"{val}")
        return ", ".join(parts)
        

    def _response_parser(self,raw_response):
        try:
            parsed_response = json.loads(raw_response)
        except Exception:
            parsed_response = raw_response
        return parsed_response
    
    def _extract_var_name(self, arg):
        match = re.search(r'\$\{([^}]+)\}', arg)
        return match.group(1) if match else arg

    def close(self):

        with open(self.REPORT_FILE, "w", encoding="utf-8") as f:
            json.dump(self.format_result(), f, indent=2, ensure_ascii=False)

        print(f"[LISTENER] Report written to {self.REPORT_FILE}")