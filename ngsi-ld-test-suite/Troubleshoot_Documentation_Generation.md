# Troubleshoot the generation of the documentation

## Missing setup step description

If a Test Case is built around permutations, you have to use the `Test Setup` (and `Test Teardown`) keywords in order
to properly generate the documentation. If you use `Suite Setup` (and `Suite Teardown`), the generated documentation
will not contain the full description of the setup step (it will not fail, but it will only contain the generic sentence).

## SyntaxWarning: invalid escape sequence '\s'

When generating the documentation, such a warning may be displayed: 

```shell
/some/path/ngsi-ld-test-suite/doc/analysis/requests.py:463: SyntaxWarning: invalid escape sequence '\s'
  regex = '(\s{4})*\s{4}\.{3}\s{4}(.*)'
```

It usually means you are calling a keyword without using named arguments (note that this does not happen for all the
keywords).

## Test Template with the same name as a Keyword

If a Test Template has the same name as a Keyword, the documentation generator will not be able to distinguish
between the two and will fail generating the documentation with such an error:

```
Traceback (most recent call last):
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/generateDocumentationData.py", line 77, in <module>
    resulting_json = create_json_of_robotfile(robot_file_tbp)
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/generateDocumentationData.py", line 44, in create_json_of_robotfile
    data.parse_robot()
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 124, in parse_robot
    _ = [self.visit_test(test=x) for x in self.suite.tests]
         ^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 460, in visit_test
    then = self.robot.get_checks(test_name=test.template, apiutils=self.apiutils, name=test.name)
           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/parserobotfile.py", line 209, in get_checks
    operation = self.get_operation_of_the_check(test_content=test_content, param=attributes)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/parserobotfile.py", line 238, in get_operation_of_the_check
    index = [x for x in index if x != -1][0]
            ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~^^^
IndexError: list index out of range
```

Make sure to use unique names for Test Templates and Keywords.

## Test Template with special characters

Special characters such as `*`, `,` or `?` are not allowed in Test Template names. Only use alphanumeric characters and spaces.

Using a special character will result in such a failure when generating the documentation:

```
Traceback (most recent call last):
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/generateDocumentationData.py", line 77, in <module>
    resulting_json = create_json_of_robotfile(robot_file_tbp)
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/generateDocumentationData.py", line 43, in create_json_of_robotfile
    data = GenerateRobotData(robot_file=robot_file, execdir=folder_test_suites)
           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 17, in __init__
    self.robot = ParseRobotFile(filename=robot_file, execdir=execdir, config_file=self.config_variables)
                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/parserobotfile.py", line 27, in __init__
    self.get_test_cases()
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/parserobotfile.py", line 96, in get_test_cases
    self.get_test_cases_with_template()
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/parserobotfile.py", line 106, in get_test_cases_with_template
    self.get_template_content(string=self.string_test_template)
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/parserobotfile.py", line 125, in get_template_content
    self.string_test_template = [x for x in subdata if self.test_template_name in x][0]
                                ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~^^^
IndexError: list index out of range
```

## Naming of permutations

The names of permutations must start with the identifier of the Test Case.

For instance, if a Test Case is named `D011_04_inc.robot`, the permutations must start with `D011_04_inc`
(for instance `D011_04_inc_01`, and not `D011_04_01_inc`).

## Test Template documentation

Always add a `[Documentation]` section to the Test Template keyword.

Even though it does not seem to be used in the generated JSON file, the generation of the documentation will
fail with such an error if it is not present:

```
Traceback (most recent call last):
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/generateDocumentationData.py", line 77, in <module>
    resulting_json = create_json_of_robotfile(robot_file_tbp)
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/generateDocumentationData.py", line 44, in create_json_of_robotfile
    data.parse_robot()
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 123, in parse_robot
    self.start_suite()
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 423, in start_suite
    reference, clauses = self.generate_reference(version=version)
                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 546, in generate_reference
    _, _ = self.generate_reference_template(version=version, need_tags=False)
           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 561, in generate_reference_template
    reference, clauses = self.get_info_from_template(name=template_name,
                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 601, in get_info_from_template
    self.documentation_template = self.documentation_template[1:][0]
                                  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~^^^
IndexError: list index out of range
```

## Naming of the response variable from the main call

Always use `response` as the name of the variable holding the response from the main call (the one being the subject of the Test Case).
The documentation generator will fail with such an error if you use a different name:

```
Traceback (most recent call last):
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/generateDocumentationData.py", line 77, in <module>
    resulting_json = create_json_of_robotfile(robot_file_tbp)
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/generateDocumentationData.py", line 44, in create_json_of_robotfile
    data.parse_robot()
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 126, in parse_robot
    _ = [self.get_step_data(test=x.name) for x in self.suite.tests]
         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 259, in get_step_data
    request, params = self.get_params(test_case=string)
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/Users/bobeal/egm/dev/datahub/ngsi-ld-test-suite/doc/analysis/generaterobotdata.py", line 210, in get_params
    response_to_check = lines_starting_response[0]
                        ~~~~~~~~~~~~~~~~~~~~~~~^^^
IndexError: list index out of range
```