import re


class ParseApiUtilsFile:
    def __init__(self, filename: str):
        with open(filename, 'r') as file:
            # Read the contents of the file
            self.file_contents = file.read()

        self.variables = dict()

        self.get_variables_data()

    def get_response(self, keyword):
        verb = str()
        url = list()
        query_param = False

        string = self.get_substring(initial_string=keyword, final_string='RETURN', include=True)
        index = string.find('    ${response}')
        string = string[index:]
        string = string.split('\n')

        for i, item in enumerate(string):
            if 'response' in item:
                regex = r"\s{4}\$\{response\}=\s{4}(POST|GET|PUT|PATCH|DELETE).*"
                match = re.match(pattern=regex, string=item)

                if match:
                    verb = match.groups()[0]
            if 'url' in item:
                url, query_param = self.get_url_request(url=item)

        return verb, url, query_param

    @staticmethod
    def get_url_request(url: str) -> [list, bool]:
        # We have two options, the url is defined in the same line of the response, or it is defined in the following
        # lines with '...'
        keys = list()
        parameters = list()
        query_param = False

        if 'response' in url:
            url = [x for x in url.split('    ') if 'url' in x][0]

        regex = r"\s*\.*\s*url=\$\{url\}\/(.*)"

        match = re.match(pattern=regex, string=url)
        if match:
            aux = match.groups()[0]

            # We need to extract the url parameters first
            if aux.find('?') != -1:
                parameters = aux.split('?')
                aux = parameters[0]
                query_param = True

            keys = re.split(r'[$/]', aux)
            keys = [k for k in keys if k != '']

            if len(parameters) != 0:
                keys.append("".join(parameters[1:]))
        else:
            regex = r"\s*\.*\s*url=\$\{temporal_api_url\}\/(.*)"

            match = re.match(pattern=regex, string=url)
            if match:
                aux = match.groups()[0]
                keys = re.split(r'[$/]', aux)
                keys = [k for k in keys if k != '']

        return keys, query_param

    def get_variables_data(self):
        string = self.get_substring(initial_string='*** Variables ***', final_string='*** ', include=False)

        self.get_variables_data_variables(string=string)
        self.get_variables_data_dictionaries(string=string)

    def get_variables_data_variables(self, string):
        # Get the simple variables from the file
        regex = r"^(\$\{.*\})\s*(.*)\n"

        matches = re.finditer(regex, string, re.MULTILINE)
        for match in matches:
            # Check that we have two groups matched
            if len(match.groups()) == 2:
                if match.group(1) not in self.variables.keys():
                    self.variables[match.group(1)] = match.group(2)
            else:
                raise Exception("Error, the variable is not following the format ${thing} = <value>")

    def get_variables_data_dictionaries(self, string):
        # Get the dictionary variables from the file
        regex = r'(\&\{.*\})'
        matches = re.finditer(regex, string, re.MULTILINE)
        for match in matches:
            # Check that we have two groups matched
            if len(match.groups()) == 1:
                # We need to find in the string the index to know the next lines from which extract the data
                key_dict = match.group(1).replace("&", "$")

                index = string.find(match.group(1))
                aux = string[index:]
                index = aux.find('\n')
                aux = aux[index:]
                index = aux.find('\n${')
                aux = aux[:index + 1]

                regex = r'\.{3}[ ]*([a-zA-Z]+)=(.*)\n'
                matches2 = re.finditer(regex, aux, re.MULTILINE)
                dict_values = dict()
                for match2 in matches2:
                    # Check that we have two groups matched
                    if len(match2.groups()) == 2:
                        key = match2.group(1)
                        value = match2.group(2)

                        value_is_variable = re.match(pattern=r'\$\{.*\}', string=value)
                        if value_is_variable:
                            value = self.variables[value]

                        dict_values[key] = value

                self.variables[key_dict] = dict_values
            else:
                raise Exception("Error, the variable is not following the format ${thing} = <value>")

    def get_substring(self, initial_string: str, final_string: str, include: bool) -> str:
        index_start = self.file_contents.find(initial_string + '\n')

        if include:
            string = self.file_contents[index_start:]
        else:
            string = self.file_contents[index_start+len(initial_string):]

        index_end = string.find(final_string)
        string = string[:index_end]

        return string
