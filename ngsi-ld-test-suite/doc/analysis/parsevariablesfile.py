from os.path import dirname


class ParseVariablesFile:
    def __init__(self):
        folder = dirname(dirname(dirname(__file__)))
        filename = f'{folder}/resources/variables.py'
        self.variables = dict()

        with open(filename, 'r') as file:
            # Read the contents of the file
            file_content = file.read()

        # Generate a list of lines from the file content
        file_content = file_content.split('\n')

        # Dismiss the blank lines and the lines starting with # -> comments
        file_content = [x for x in file_content if x != '' and x[0] != '#']

        # Extract the key = value format of the variables
        file_content = [x.split('=') for x in file_content if x != '']

        # Delete the ' characters from the keys and delete blank spaces
        self.variables = {x[0].strip(): x[1].replace("'", "").strip() for x in file_content}

    def get_variable(self, variable: str) -> str:
        # We request the variable in the form '${...}'
        variable = variable.strip('${}')
        value = self.variables[variable]

        return value
