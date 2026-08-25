from requests import post, get
from re import finditer
from json import loads

try:
    from resources.variables import github_owner, github_broker_repo, github_token
except ImportError:
    # Some of the variables were not defiled, therefore we cannot execute the operation
    classError = True
else:
    classError = False


class GitHubIssue:
    def __init__(self, issue_title: str, issue_content: str):
        if classError:
            # There is some GitHub parameters not defined, therefore this function does not effect
            print("\nSome GitHub parameters were not defined in variables.py")
            print("Expected parameters: github_owner, github_broker_repo, github_token")
            return
        else:
            # Get the values of the parameter from the variables.py file
            # GitHub repository details
            self.url_create = f'https://api.github.com/repos/{github_owner}/{github_broker_repo}/issues'

            self.issue_title = issue_title
            self.issue_content = issue_content

            self.test_cases = list()
            self.test_cases_title = list()

    def create_issues(self):
        if classError:
            # There is some GitHub parameters not defined, therefore this function does not effect
            print("\nSome GitHub parameters were not defined in variables.py")
            print("Expected parameters: github_owner, github_broker_repo, github_token")
            return
        else:
            # Request body, the issue content need to be split into the different Test Cases in order to prevent
            # body maximum of 65536 characters
            self.generate_test_cases_info()

            for i in range(0, len(self.test_cases_title)):
                # We need to check that the issue was not already created previously
                # if the issue is created previously and still open we do not create again,
                # other case, we create the issue

                # Obtain the extended title of the issue
                issue_title = f'{self.issue_title} {self.test_cases_title[i]}'

                # Check the issue
                if self.check_duplicate_issue(issue_title=issue_title):
                    print('\nDuplicate issue found!')
                else:
                    self.create_issue(body=self.test_cases[i])

    def create_issue(self, body: str):
        if classError:
            # There is some GitHub parameters not defined, therefore this function does not effect
            print("\nSome GitHub parameters were not defined in variables.py")
            print("Expected parameters: github_owner, github_broker_repo, github_token")
            return
        else:
            # Issue details
            # Data of the issue
            data = {
                'title': self.issue_title,
                'body':  body
            }

            # Request headers
            headers = {
                'Accept': 'application/vnd.github.v3+json',
                'Authorization': f'Token {github_token}'
            }

            # Send POST request to create the issue
            response = post(url=self.url_create, headers=headers, json=data)

            # Check the response status code
            if response.status_code == 201:
                print('\nIssue created successfully.')
            else:
                print('\nFailed to create the issue.')
                print(f'Response: {response.status_code} - {response.text}')

    def generate_test_cases_info(self):
        if classError:
            # There is some GitHub parameters not defined, therefore this function does not effect
            print("\nSome GitHub parameters were not defined in variables.py")
            print("Expected parameters: github_owner, github_broker_repo, github_token")
            return
        else:
            pattern = r'##\s*[0-9]{3}_[0-9]{2}_[0-9]{2}.*\n'  # Split on one or more non-word characters

            count = int()
            indexes = list()

            match = None
            for match in finditer(pattern, self.issue_content):
                count += 1
                indexes.append(match.start())

            if match:
                title = self.issue_content[0:indexes[0]]
            else:
                raise KeyError("Search unsuccessful. It was expected the the name of the Test Cases start with "
                               "<ddd>_<dd>_<dd>, where d is a digit, e.g., 027_01_01")

            # Get the list of Test Cases
            for i in range(1, len(indexes) + 1):
                self.test_cases_title.append(f'({self.issue_content[indexes[i-1]+3:indexes[i-1]+12]})')

                if i < len(indexes):
                    self.test_cases.append(self.issue_content[indexes[i-1]:indexes[i]])
                else:
                    self.test_cases.append(self.issue_content[indexes[i-1]:])

            self.test_cases = [f'{title}\n\n{x}' for x in self.test_cases]

    def check_duplicate_issue(self, issue_title):
        if classError:
            # There is some GitHub parameters not defined, therefore this function does not effect
            print("\nSome GitHub parameters were not defined in variables.py")
            print("Expected parameters: github_owner, github_broker_repo, github_token")
            return
        else:
            # Generate the URL of the query
            url = f'repo:{github_owner}/{github_broker_repo} is:issue is:open in:title "{issue_title}"'

            # Make the API request
            response = get(
                'https://api.github.com/search/issues',
                params={'q': url}
            )

            # Check the response status code
            if response.status_code == 200:
                # Parse the JSON response
                data = response.json()

                # Check if any issues were found
                if data['total_count'] > 0:
                    return True  # Duplicate issue found
            else:
                raise Exception(loads(response.text)['errors'][0]['message'])

            return False  # No duplicate issue found
