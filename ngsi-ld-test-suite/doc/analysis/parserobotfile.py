import re
import os
from analysis.checks import Checks
from analysis.requests import Requests


class ParseRobotFile:
    def __init__(self, filename: str, execdir: str, config_file):
        self.test_suite = os.path.basename(filename).split('.')[0]
        self.filename = filename

        with open(filename, 'r') as file:
            # Read the contents of the file
            self.file_contents = file.read()

        self.variables = dict()
        self.execdir = execdir
        self.resource_files = list()
        self.apiutils = list()

        self.string_test_template = str()
        self.test_template_name = str()
        self.template_params_value = dict()
        self.check_template()
        self.file_type = self._detect_file_type()

        # Parse based on file type
        if self.file_type == "IOP":
            self.settings_documentation = self.get_iop_settings_documentation()
            self.get_iop_variables_data()
            self.get_iop_test_cases()
        else:  # TP
            self.get_variables_data()
            self.get_apiutils_path()
            self.get_test_cases()

        self.config_file = config_file

    def check_template(self):
        # Check if there is a template and which template we have
        aux = re.findall(pattern=r'Test Template[ ]+(.*)', string=self.file_contents)

        if len(aux) != 0:
            self.test_template_name = aux[0]
        else:
            self.test_template_name = ''

    def _detect_file_type(self) -> str:
        """
        Detect if this is an IOP or TP (Test Procedure) file.
        Returns:
            "IOP" or "TP"
        """
        if 'IOP_TP' in self.filename:
            return "IOP"
        
        return "TP"

    def set_apiutils(self, apiutils):
        self.apiutils = apiutils

    def get_variables_data(self):
        string = self.get_substring(initial_string='*** Variables ***\n', final_string='*** ', include=False)

        regex = r"(\$\{.*\})\s*=\s*(.*)\n"

        matches = re.finditer(regex, string, re.MULTILINE)
        for match in matches:
            # Check that we have two groups matched
            if len(match.groups()) == 2:
                self.variables[match.group(1)] = match.group(2)
            else:
                raise Exception("Error, the variable is not following the format ${thing} = <value>")

    def get_apiutils_path(self):
        string = self.get_substring(initial_string='Resource', final_string='*** Variables ***', include=True)
        result = [item for item in string.split('\n') if 'ApiUtils' in item and item[0] != '#']
        self.resource_files = [x.replace('${EXECDIR}', self.execdir) for x in result]

        regex = r"\s*Resource\s*(.*)\s*"
        result = [re.match(pattern=regex, string=x).group(1) for x in result]
        self.resource_files = [x.replace('${EXECDIR}', self.execdir) for x in result]

        # if len(result) >= 1:
        #     regex = r"\s*Resource\s*(.*)\s*"
        #
        #     matches = re.finditer(regex, result[0], re.MULTILINE)
        #     for match in matches:
        #         # Check that we have 1 group matched
        #         if len(match.groups()) == 1:
        #             self.resource_file.append(match.group(1))
        #         else:
        #             raise Exception("Error, unexpected format")
        #
        #    self.resource_file = self.resource_file.replace('${EXECDIR}', self.execdir)

    def get_substring(self, initial_string: str, final_string: str, include: bool) -> str:
        index_start = self.file_contents.find(initial_string)

        if include:
            string = self.file_contents[index_start:]
        else:
            string = self.file_contents[index_start+len(initial_string):]

        if final_string != '':
            index_end = string.find(final_string)
            string = string[:index_end]

        return string

    def get_test_cases(self):
        if self.test_template_name == '':
            self.get_test_cases_without_template()
        else:
            self.get_test_cases_with_template()

    def get_test_cases_with_template(self):
        # In case of template, we have to get the value of the tags and the parameters value from the Test Cases and
        # the content of the operations from the corresponding Keyword content corresponding to the Test Template
        index_start_test_cases = self.file_contents.find('*** Test Cases ***')
        index_start_keywords = self.file_contents.find('*** Keywords ***')

        string_test_cases = self.file_contents[index_start_test_cases+len('*** Test Cases ***')+1:index_start_keywords]
        self.string_test_template = self.file_contents[index_start_keywords+len('*** Keywords ***')+1:]
        self.get_template_content(string=self.string_test_template)

        self.get_test_cases_content(string=string_test_cases)
        self.get_template_param_values(test_cases=string_test_cases)

    def get_template_content(self, string: str):
        matches = re.findall(pattern=r'^(([a-zA-z0-9\-\/@]+[ ]*)+)$', string=string, flags=re.MULTILINE)

        indexes = list()
        for match in matches:
            indexes.append(string.rfind(match[0]))

        subdata = list()
        for i in range(0, len(indexes) - 1):
            subdata.append(string[indexes[i]:indexes[i + 1]])

        index = indexes[len(indexes) - 1]
        subdata.append(string[index:])

        self.string_test_template = [x for x in subdata if self.test_template_name in x][0]

    def get_template_param_values(self, test_cases):
        # Extract the parameter of the Test Cases
        # the first line is the argument of the test case
        params = test_cases.split("\n")[0].strip().split("    ")
        params = [x.lower() for x in params]

        # Obtain the params values
        keys = self.test_cases.keys()
        data = self.test_cases

        for k in keys:
            aux = data[k]
            aux = aux.split('\n')[1:]
            aux = [x.strip() for x in aux if x.find("[Tags]") == -1 and x.find("[Documentation]") == -1][0]
            aux = aux.split("    ")

            result = [{f"${{{k}}}": v} for k, v in zip(params, aux)]
            result = {k: v for d in result for k, v in d.items()}

            self.template_params_value[k] = result

    def get_test_cases_content(self, string):
        pattern = f'{self.test_suite}_\d+[ ]+.*'
        matches = re.findall(pattern=pattern, string=string)

        indexes = list()
        self.test_case_names = list()
        if matches:
            for match in matches:
                name = match.strip()
                self.test_case_names.append(name)
                indexes.append(string.find(name))
        else:
            # The test case has the same id. number as the test suite
            pattern = f'{self.test_suite}\s.*'
            matches = re.findall(pattern=pattern, string=string)

            for match in matches:
                name = match.strip()
                self.test_case_names.append(name)
                indexes.append(string.find(name))

        self.test_cases = dict()
        for i in range(0, len(indexes)-1):
            self.test_cases[self.test_case_names[i]] = string[indexes[i]:indexes[i+1]]

        try:
            self.test_cases[self.test_case_names[-1]] = string[indexes[-1]:]
        except IndexError:
            raise Exception(f"ERROR, List index out of range, "
                            f"probably the name of the Test Case is not following the pattern '{pattern}'")

    def get_test_cases_without_template(self):
        index_start_test_cases = self.file_contents.find('*** Test Cases ***')
        index_start_keywords = self.file_contents.find('*** Keywords ***')

        string_test_cases = self.file_contents[index_start_test_cases+len('*** Test Cases ***')+1:index_start_keywords]

        self.get_test_cases_content(string=string_test_cases)

    def get_text_cases_content(self, name: str):
        if self.test_template_name == '':
            result = self.test_cases[name]
        else:
            result = self.string_test_template

        return result

    def get_checks(self, test_name, apiutils, name):
        data = Checks()
        self.test_name = test_name
        self.test_case_name = name

        # test_content = self.test_cases[test_name]
        test_content = self.get_text_cases_content(name=test_name)

        # Get The lines starting by 'Check'
        checks = list()
        param = dict()
        lines_starting_with_check = self.get_lines_with_checks(content=test_content)
        for line in lines_starting_with_check:
            check, param, attributes = self.get_data_check(test_case=test_content, checks=data, line=line)
            operation = self.get_operation_of_the_check(test_content=test_content, param=attributes)
            result = data.get_checks(checks=check, **param)

            content = {
                'operation': operation,
                'checks': result
            }

            checks.append(content)

        result = self.generate_then_content(content=checks)

        return result

    def get_operation_of_the_check(self, test_content: str, param: dict) -> str:
        if param == 'Notification':
            operation = param
        else:
            values = [x for x in param if x.find('${') != -1]
            values = [f"{x.split('.')[0]}}}" if x.find(".") != -1 else x for x in values]

            values = [x for x in values if x.find('response') != -1]

            if len(values) != 0:
                # We need to find the operation of the response
                index = [test_content.find(x) for x in values]
                filtered_index = [x for x in index if x != -1]
                
                if len(filtered_index) > 0:
                    index = filtered_index[0]
                    substring = test_content[index:]
                    end_line = substring.find('\n')
                    substring = substring[0:end_line]

                    operation = substring.split('    ')[1]
                else:
                    # No response variable found in test content, default to Notification
                    operation = 'Notification'
            else:
                # We have a notification operation
                operation = 'Notification'

        return operation

    def get_lines_with_checks(self, content):
        new_list = list()

        # Obtain the complete list of lines that contains a Check
        lines_starting_with_check = re.findall(r'^\s*Check.*', content, re.MULTILINE)

        if len(lines_starting_with_check) != 0:
            # TODO: From the list of Checks, we need to discard all 'Check Response Status Code' except the last one.
            #  Should be resolve when clearly defined the Setup process of the Test Suite
            # check_string = 'Check Response Status Code'
            # lines_starting_with_check = [x.strip() for x in lines_starting_with_check]
            # new_list = [value for value in lines_starting_with_check if not value.startswith(check_string)]
            # abb_values = [value for value in lines_starting_with_check if value.startswith(check_string)]

            # if abb_values:
            #    new_list.append(abb_values[-1])
            new_list = [x.strip() for x in lines_starting_with_check]
        elif content.find('Wait for notification') != 0:
            # There is no Check, we need to check if there is a 'Wait for notification',
            # then we need to check the 'Should be Equal' sentences
            pattern = \
                (r'(Wait for no notification)|'
                 r'(Wait for notification and validate it)|'
                 r'(Wait for notification)([ ]{4}(.*))?')

            param = re.findall(pattern=pattern, string=content, flags=re.MULTILINE)
            for i in range(0, len(param)):
                data = tuple(element for element in param[i] if element != '')

                match data[0]:
                    case 'Wait for notification and validate it':
                        new_list.append(f'Wait for notification and validate it')
                    case 'Wait for no notification':
                        new_list.append(f'Wait for no notification')
                    case 'Wait for notification':
                        last_index = len(data) - 1
                        timeout = data[last_index]

                        if timeout != '':
                            new_list.append(f'Wait for notification    {timeout}')
                        else:
                            new_list.append(f'Wait for notification    5')
                    case _:
                        raise Exception(f"Unexpected Wait for notification check: '{param[i][0]}'")

            lines_starting_with_should = re.findall(r'^\s*Should be Equal.*', content, re.MULTILINE)
            _ = [new_list.append(x.strip()) for x in lines_starting_with_should]

            lines_starting_with_should = re.findall(r'^\s*Dictionary Should Contain Key.*', content, re.MULTILINE)
            _ = [new_list.append(x.strip()) for x in lines_starting_with_should]

            lines_starting_with_should = re.findall(r'^\s*Dictionary Should Not Contain Key.*', content, re.MULTILINE)
            _ = [new_list.append(x.strip()) for x in lines_starting_with_should]

            lines_starting_with_should = re.findall(r'^\s*Should Not Be Empty.*', content, re.MULTILINE)
            _ = [new_list.append(x.strip()) for x in lines_starting_with_should]

            lines_starting_with_should = re.findall(r'^\s*Should [Bb]e True.*', content, re.MULTILINE)
            _ = [new_list.append(x.strip()) for x in lines_starting_with_should]

        return new_list

    def get_request(self, test_name, name):
        flattened_list_variables = {k: v for d in self.apiutils for k, v in d.variables.items()}
        data = Requests(variables=self.variables,
                        apiutils_variables=flattened_list_variables,
                        config_file=self.config_file,
                        template_params_value=self.template_params_value,
                        test_name=test_name,
                        name=name)

        if self.test_template_name == '':
            description = data.get_description(string=self.test_cases[test_name])
        else:
            description = data.get_description(string=self.string_test_template)

        return description

    def generate_then_content(self, content):
        # Need to check if it is a Notification data or a normal Response
        aux = [x for x in content if x['checks'].find('Notification data') != -1 or
               x['checks'].find('After waiting') != -1 or
               x['checks'].find('Notification and validate') != -1]

        if len(aux) == 0:
            # The SUT sends a valid Response
            checks = [f"{x['operation']} with {x['checks']}" for x in content]
            checks = [x.replace("    ", "            ") for x in checks]
            if len(content) > 1:
                checks = "     and\n        ".join(checks)
                checks = f"then {{\n    the SUT sends a valid Response for the operations:\n        {checks}\n}}"
            elif len(content) == 1:
                checks = f"then {{\n    the SUT sends a valid Response for the operation:\n        {checks[0]}\n}}"
            else:
                raise Exception("ERROR, It is expected at least 1 Check operation in the Test Case")
        else:
            # The Client receives a valid Notification
            checks = [f"{x['operation']} received {x['checks']}" for x in content]
            if len(checks) > 1:
                checks = "     and\n        ".join(checks)
                checks = (f"then {{\n    the Test System at '${{endpoint}}' receives a valid Notification containing:\n"
                          f"        {checks}\n}}")
            elif len(content) == 1:
                checks = checks[0]
                checks = (f"then {{\n    the Test System at '${{endpoint}}' receives a valid Notification containing:\n"
                          f"        {checks}\n}}")
                # if content[0]['checks'].find('Waiting for no Notification') != -1:
                #     # Waiting for no notification data
                #     print("Error, need to control the generation of then message")
                #     exit(-1)
                #     match = re.match(pattern=r"[\W\w]+'(\d+)'", string=content[0])
                #     try:
                #         checks =
                #           f"then {{\n    the SUT will not send a CsourceNotification after {match.group(1)} seconds}}"
                #     except Exception:
                #         raise Exception(f"ERROR: unexpected timeout parameter: '{content[0]}'")
                # else:
                #     print("Error, need to control the generation of then message")
                #     exit(-1)
                #     checks =
                #       (f"then {{\n    the client at '${{endpoint}}' receives a valid Notification, {content[0]}\n}}")
            else:
                raise Exception("ERROR, It is expected at least 1 Notification Check operation in the Test Case")

        return checks

    def generate_when_content(self, http_verb, endpoint, when):
        if when.find("a subscription with id set to") == -1 and when.find("Call API Endpoint with") == -1:
            # print("http verb", http_verb, "endpoint", endpoint, "when", when)
            url = f"URL set to '/ngsi-ld/v1/{endpoint}'"
            method = f"method set to '{http_verb}'"
            when = (f"when {{\n    the SUT receives a Request from the Test System containing:\n"
                    f"        {url}\n"
                    f"        {method}\n"
                    f"        {when}\n"
                    f"}}")
        elif when.find("Call API Endpoint with") != -1:
            when = (f"when {{\n    the SUT receives a Request from the Test System containing:\n"
                    f"        {when}\n"
                    f"}}")
        else:
            # This is a Notification operation
            when = f"The Test System at ${{endpoint}} receives a valid Notification containing {when}"
        return when

    def get_data_check(self, test_case, checks, line):
        content = line.split("    ")

        # Discard lines that are comments
        # content = [x for x in content if x.strip()[0] != '#']
        aux = len(content)

        try:
            position_params = checks.args[content[0]]
            if aux == 1:
                # We are in multiline classification of the Check, need to extract the parameter for the next lines
                params, attributes = self.find_attributes_next_line(test_case=test_case,
                                                                    name=content[0],
                                                                    position_params=position_params)
                return content[0], params, attributes
            elif aux > 1:
                # We are in one line definition
                params = self.find_attributes_same_line(params=position_params, content=content[1:])

                if content[0] == 'Wait for notification' or content[0] == 'Should be Equal':
                    attributes = 'Notification'
                else:
                    attributes = content[1:]

                return content[0], params, attributes
            else:
                raise Exception("ERROR, line should contain data")
        except KeyError:
            # The Check operation does not require parameters
            return content[0], dict(), list()

    def find_attributes_same_line(self, params, content):
        result = dict()

        if len(params['position']) > 0:
            for i in range(0, len(params['position'])):
                param_key = params['params'][i]
                param_position = params['position'][i]
                param_value = self.get_param_value(position=content[param_position])
                result[param_key] = param_value
        elif len(params['position']) == 0:
            for i in range(0, len(params['params'])):
                param_key = params['params'][i]
                param_value = self.get_param_value_for_waiting(param_key=param_key, content=content)

                if param_value is not None:
                    try:
                        # This is a parameter of the template, therefore we take the value of the param
                        result[param_key] = self.template_params_value[self.test_case_name][f'${{{param_value}}}']
                    except KeyError:
                        # Check if it is a parameter of the Test Suite
                        try:
                            result[param_key] = self.variables[f'${{{param_value}}}']
                        except KeyError:
                            result[param_key] = param_value

        return result

    def get_param_value_for_waiting(self, param_key, content):
        found = [x for x in content if x.find(param_key) != -1]
        length = len(found)

        if length != 0:
            found = found[0]

            if found.find('=') != -1:
                # in the format variable=${value}
                # pattern = f"{param_key}=\${{(\d+)}}"
                pattern = f"{param_key}=\${{([\w\W]+)}}|{param_key}=([\w\W]+)"
            else:
                pattern = f"\${{([\w\W]+)}}|([\w\W]+)"

        # elif length == 0 and len(content) > 1:
        #     # There is params, but they are not written in the form key=value
        #     found = content[1]
        #     # pattern = f"\${{(\d+)}}"
        #     pattern = f"\${{([\w\W]+)}}|([\w\W]+)"
        else:
            pattern = ''
            found = ''
            return

        value = re.match(pattern=pattern, string=found)

        try:
            aux = value.group(2)
            if aux is None:
                aux = value.group(1)
            value = aux
        except AttributeError:
            value = ''

        return value

    def find_attributes_next_line(self, test_case, name, position_params):
        index_start = test_case.find(name)

        aux = test_case[index_start+len(name)+1:].split('\n')

        params = list()
        attributes = list()
        for a in range(0, len(aux)):
            param = aux[a]
            # if param.startswith("    ..."):
            #     data = param.split('    ')[-1]
            #     params.append(data)
            #     if data.find('=') != -1:
            #         data = data.split('=')[-1]
            #
            #     attributes.append(data)
            # else:
            #     break

            regex = '(\s{4})*\s{4}\.{3}\s{4}(.*)'
            data = re.match(pattern=regex, string=param)
            if data:
                data = data.groups()[-1]
                params.append(data)

                if data.find('=') != -1:
                    data = data.split('=')[-1]

                attributes.append(data)
            else:
                break

        param = self.find_attributes_same_line(params=position_params, content=params)
        return param, attributes

    def get_param_value(self, position):
        try:
            # Check if we can get the data from the current robot files
            result = self.variables[position]
        except KeyError:
            try:
                # Check if we can get the data from the apiutils file
                # TODO: this operation is calculated every time that wanted to calculate this operation
                flattened_list = {k: v for d in self.apiutils for k, v in d.variables.items()}
                result = flattened_list[position]
            except KeyError:
                try:
                    aux = re.findall(pattern=r'\$\{(.*)}', string=position)
                    if len(aux) != 0:
                        aux = aux[0]
                    else:
                        aux = position
                    result = self.config_file.get_variable(aux)
                except KeyError:
                    try:
                        aux1 = self.template_params_value[self.test_case_name]
                        result = aux1[f'${{{aux}}}']

                        try:
                            if result[:2] == "${":
                                result = self.get_param_value(result)
                        except KeyError:
                            result = f'${{{aux}}}'
                    except KeyError:
                        if '$' in position:
                            return position
                        else:
                            result = aux

        return result
    
    def get_iop_settings_documentation(self) -> str:
        """Extract Documentation from *** Settings *** section for IOP files."""
        string = self.get_substring(initial_string='*** Settings ***\n', final_string='*** Variables ***', include=False)
        pattern = r'Documentation\s+(.*?)(?=\n)'
        match = re.search(pattern, string, re.MULTILINE)
        if match:
            return match.group(1).strip()
        return ""
    
    def get_iop_test_setup(self) -> str:
        """Extract Test Setup from *** Settings *** section (suite-level for IOP files)."""
        string = self.get_substring(initial_string='*** Settings ***\n', final_string='*** Variables ***', include=False)
        pattern = r'Test Setup\s+(.*?)(?=\n)'
        match = re.search(pattern, string, re.MULTILINE)
        if match:
            return match.group(1).strip()
        return ""
    
    def get_iop_test_teardown(self) -> str:
        """Extract Test Teardown from *** Settings *** section (suite-level for IOP files)."""
        string = self.get_substring(initial_string='*** Settings ***\n', final_string='*** Variables ***', include=False)
        pattern = r'Test Teardown\s+(.*?)(?=\n)'
        match = re.search(pattern, string, re.MULTILINE)
        if match:
            return match.group(1).strip()
        return ""
    
    def get_iop_variables_data(self):
        """Extract variables from *** Variables *** section for IOP files."""
        string = self.get_substring(initial_string='*** Variables ***\n', final_string='*** ', include=False)

        # Pattern: ${VAR_NAME}    value
        regex = r"\$\{([^}]+)\}\s+(.+?)(?=\n|$)"
        matches = re.finditer(regex, string, re.MULTILINE)
        
        for match in matches:
            if len(match.groups()) == 2:
                var_name = match.group(1)
                var_value = match.group(2).strip()
                self.variables[f'${{{var_name}}}'] = var_value

    def get_iop_test_cases(self):
        """Extract test cases from *** Test Cases *** section for IOP files."""
        index_start_test_cases = self.file_contents.find('*** Test Cases ***')
        if index_start_test_cases == -1:
            self.test_cases = dict()
            self.test_case_names = list()
            return
        
        index_start_keywords = self.file_contents.find('*** Keywords ***')
        if index_start_keywords == -1:
            string_test_cases = self.file_contents[index_start_test_cases + len('*** Test Cases ***') + 1:]
        else:
            string_test_cases = self.file_contents[index_start_test_cases + len('*** Test Cases ***') + 1:index_start_keywords]

        # Extract test case names
        pattern = r'^([A-Z0-9_]+[ ].*)$'
        matches = list(re.finditer(pattern, string_test_cases, re.MULTILINE))

        if not matches:
            self.test_cases = dict()
            self.test_case_names = list()
            return

        # Extract positions and names
        indexes = [match.start() for match in matches]
        self.test_case_names = [match.group(1).strip() for match in matches]

        # Extract test case content
        self.test_cases = dict()
        for i in range(len(indexes) - 1):
            content = string_test_cases[indexes[i]:indexes[i + 1]]
            self.test_cases[self.test_case_names[i]] = content

        # Add last test case
        if len(indexes) > 0:
            self.test_cases[self.test_case_names[-1]] = string_test_cases[indexes[-1]:]

    def _get_documentation_content(self, test_name: str) -> str:
        """Helper: Extract raw documentation content after [Documentation] until next keyword block."""
        if test_name not in self.test_cases:
            return ""
        
        test_content = self.test_cases[test_name]
        match = re.search(r'\[Documentation\]\s*', test_content)
        if not match:
            return ""
        
        start_pos = match.end()
        doc_section = re.search(r'(.*?)(?=\n\s*\[|\Z)', test_content[start_pos:], re.MULTILINE | re.DOTALL)
        return doc_section.group(1) if doc_section else ""

    def get_iop_documentation_data(self, test_name: str) -> dict:
        """Extract documentation in the Test Cases."""
        if test_name not in self.test_cases:
            return " "
        
        doc_content = self._get_documentation_content(test_name)
        if not doc_content:
            return " "
        
        # Split by LABEL: pattern and capture the labels
        parts = re.split(r'([A-Za-z\s\-]+):\s*', doc_content)
        sections = {
            parts[i].strip().lower().replace(' ', '_').replace('-', '_'):
            re.sub(r'\n\s*\.{3}\s*', ' ', parts[i + 1].strip()).replace('\n', ' ').strip().rstrip('.')
            for i in range(1, len(parts), 2) if i + 1 < len(parts)
        }

        return sections

    def get_iop_test_tags(self, test_name: str) -> list:
        """Extract [Tags] from a specific IOP test case."""
        if test_name not in self.test_cases:
            return []

        test_content = self.test_cases[test_name]
        pattern = r'\[Tags\]\s*(.*?)(?=\n\s+\[|\n\s*$)'
        match = re.search(pattern, test_content, re.MULTILINE | re.DOTALL)

        if not match:
            return []

        tags_text = match.group(1)
        return [tag.strip() for tag in tags_text.split() if tag.strip()]

    def get_iop_comments(self, test_name: str) -> list:
        """Extract comments from a specific IOP test case."""
        if test_name not in self.test_cases:
            return []

        test_content = self.test_cases[test_name]
        comments = re.findall(r'^\s*#(.+?)$', test_content, re.MULTILINE)
        return [comment.strip() for comment in comments]

    def get_iop_setup(self, test_name: str) -> str:
        """Extract [Setup] from a specific IOP test case."""
        if test_name not in self.test_cases:
            return ""

        test_content = self.test_cases[test_name]
        pattern = r'\[Setup\]\s*(.*?)(?=\n\s+\[|\n\s*$)'
        match = re.search(pattern, test_content, re.MULTILINE | re.DOTALL)

        if not match:
            return ""

        setup_text = match.group(1).strip()
        return setup_text

    def get_iop_teardown(self, test_name: str) -> str:
        """Extract [Teardown] from a specific IOP test case."""
        if test_name not in self.test_cases:
            return ""

        test_content = self.test_cases[test_name]
        pattern = r'\[Teardown\]\s*(.*?)(?=\n\s+\[|\n\s*$)'
        match = re.search(pattern, test_content, re.MULTILINE | re.DOTALL)

        if not match:
            return ""

        teardown_text = match.group(1).strip()
        return teardown_text

    def get_iop_test_info(self, test_name: str) -> dict:
        """Get all information for a specific IOP test case."""
        return {
            'tp_id': test_name,
            'documentation': self.get_iop_test_documentation(test_name),
            'tags': self.get_iop_test_tags(test_name),
            'comments': self.get_iop_comments(test_name),
            'variables': self.variables,
            'resource_files': self.resource_files,
            'test_cases': self.test_case_names
        }
