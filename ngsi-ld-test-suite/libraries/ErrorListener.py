from os.path import join, splitext, exists
from os import getcwd, remove
from re import compile, match, MULTILINE
from json import loads, dumps
from http import HTTPStatus
from convertMD import Markdown
from githubIssue import GitHubIssue
from robot.running.context import EXECUTION_CONTEXTS


def __get_header__(dictionary: dict, key: str) -> str:
    result = str()
    try:
        result = f'  {key}: {dictionary["headers"][key]}\n'
        return result
    except KeyError:
        pass


def __get_status_meaning__(status_code):
    try:
        status = HTTPStatus(status_code)
        return status.phrase
    except ValueError:
        return "Unknown status code"


def __is_string_dict__(string: str) -> bool:
    try:
        json_object = loads(string)
        if isinstance(json_object, dict):
            return True
    except ValueError:
        pass
    return False


def __flatten_concatenation__(matrix):
    flat_list = []
    for row in matrix:
        if isinstance(row, str):
            flat_list.append(row)
        else:
            flat_list += row

    return flat_list


def __get_body__(dictionary: dict):
    result = str()
    if dictionary is None:
        result = '  No body\n'
    else:
        result = dumps(dictionary, indent=2)
        result = (result.replace('\n', '\n  ')
                  .replace("{", "  {")
                  .replace("[", "  [") + '\n')

    return result


class ErrorListener:
    ROBOT_LISTENER_API_VERSION = 2

    def __init__(self, filename='errors.log'):
        self.filename_md = None
        self.filename_log = None
        self.cwd = None
        self.outfile = None
        self.previous_content = str()
        self.filename = filename

        self.max_length_suite = 150
        self.max_length_case = 80
        self.tests = list()
        self.suite_name = str()
        self.rx_dict = {
            'variables': compile('^\${.*$|^\&{.*$|^\@{.*'),
            'http_verbs': compile('^GET.*(Request|Response).*$|'
                                  '^HEAD.*(Request|Response).*$|'
                                  '^POST.*(Request|Response).*$|'
                                  '^PUT.*(Request|Response).*$|'
                                  '^DELETE.*(Request|Response).*$|'
                                  '^CONNECT.*(Request|Response).*$|'
                                  '^OPTIONS.*(Request|Response).*$|'
                                  '^TRACE.*(Request|Response).*$|'
                                  '^PATCH.*(Request|Response).*$', MULTILINE),
            'length_log': compile('^Length is \d+$')
        }

    def generate_file_path(self):
        if self.outfile is None:
            # This is the first time that we execute the test therefore we configure the filenames
            ns = EXECUTION_CONTEXTS.current
            output_dir = ns.variables.current.store.data['OUTPUT_DIR']

            basename = splitext(self.filename)[0]
            self.filename_log = join(output_dir, self.filename)
            self.filename_md = join(output_dir, f'{basename}.md')

            self.cwd = getcwd()
            self.outfile = open(self.filename_log, 'w')

            # Check if a previous version of the markdown file exists in the folder, then we delete it in order
            # not to append to this file
            if exists(self.filename_md):
                remove(self.filename_md)

    def start_suite(self, name, attrs):
        self.generate_file_path()

        self.suite_name = attrs['source'].replace(self.cwd, '')[1:].replace('.robot', '').replace('/', ".")

        if attrs['doc'] != '':
            self.outfile.write(f'{"=" * self.max_length_suite}\n')
            self.outfile.write(f'{self.suite_name} :: {attrs["doc"]}\n')
            self.outfile.write(f'{"=" * self.max_length_suite}\n')

    def start_test(self, name, attrs):
        self.tests.append(f"\n\n{name}\n")
        self.tests.append(f'{"=" * self.max_length_case}\n')

    def end_test(self, name, attrs):
        if attrs['status'] != 'PASS':
            flat_list = __flatten_concatenation__(matrix=self.tests)
            [self.outfile.write(x) for x in flat_list if x is not None]
            self.tests.clear()

    def end_suite(self, name, attrs):
        self.outfile.write('\n\n\n')
        self.outfile.close()

        try:
            # If there was an error, generate the markdown content and upload an issue in the corresponding
            # GitHub Repository
            md = Markdown(filename=self.filename_log, previous_content=self.previous_content)
            md.get_names()
            # md.generate_md()
            self.previous_content = md.save_file(filename=self.filename_md)

            # Check if we have defined the GitHub parameters in the variables.py file, if this is the case upload
            # gh = GitHubIssue(issue_title=f'{attrs["longname"]} - {attrs["doc"]}', issue_content=md.get_markdown())

            # gh.create_issues()
        except KeyError as err:
            print(f'\n[ERROR] Unexpected {err=}, {type(err)=} in TC {self.suite_name}\n\n')
        except IndexError as err:
            print(f'\n[ERROR] Unexpected {err=}, {type(err)=} in TC {self.suite_name}\n\n')
        except Exception as err:
            print(f'\n[ERROR] Unexpected {err=}, {type(err)=} in TC {self.suite_name}\n\n')

        # We need to reopen the file in case that we are executing several TCs
        self.outfile = open(self.filename_log, 'a')

    def log_message(self, msg):
        if (not match(pattern=self.rx_dict['variables'], string=msg['message']) and
                not match(pattern=self.rx_dict['http_verbs'], string=msg['message'])):
            self.tests.append(self.__get_message__(msg["message"]))

    def close(self):
        self.outfile.write('\n\n\n')
        self.outfile.close()

    def __get_message__(self, message: str) -> str:
        result = str()
        if message == 'Request ->':
            result = f'\n\nRequest:\n{"-" * self.max_length_case}\n'
        elif message == 'Response ->':
            result = f'\n\nResponse:\n{"-" * self.max_length_case}\n'
        elif __is_string_dict__(string=message):
            result = self.__generate_pretty_output__(data=message)
        elif message[0] == '\n':
            # This is the title of a test case operation
            result = message
        elif message == 'Dictionary comparison failed with -> ':
            result == None
        elif match(pattern=self.rx_dict['length_log'], string=message) is None:
            result = f'\nMismatch:\n{"-" * self.max_length_case}\n{message}\n'

        return result

    def __generate_pretty_output__(self, data: str) -> list:
        data = loads(data)

        output = list()

        received_header_keys = data['headers'].keys()

        if 'User-Agent' in received_header_keys:
            # User-Agent is a Request Header, therefore we generate the request header
            output.append(f'  {data["method"]} {data["url"]}\n')

            [output.append(__get_header__(dictionary=data, key=x)) for x in list(received_header_keys)]

            output.append('\n')

            output.append(__get_body__(dictionary=data['body']))
        else:
            # This is a Response header
            # robotframework-requests is based on python request, so it is using HTTP/1.1
            output.append(f'  HTTP/1.1 {data["status_code"]} {__get_status_meaning__(data["status_code"])}\n')

            [output.append(__get_header__(dictionary=data, key=x)) for x in list(received_header_keys)]

            output.append(f'  Date: REGEX(. *)\n')

            output.append('\n')

            output.append(__get_body__(dictionary=data['body']))

        return output
