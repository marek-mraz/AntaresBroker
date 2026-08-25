# Contributing to the ETSI NGSI-LD Test Suite

Thank you for contributing! This document explains the conventions and rules all contributors are expected to follow.

---

## Table of Contents

- [IDE setup](#ide-setup)
- [Commit messages](#commit-messages)
- [Pre-commit hook](#pre-commit-hook)
- [Authoring a new Test Case](#authoring-a-new-test-case)
  - [File and naming conventions](#file-and-naming-conventions)
  - [Test Case template](#test-case-template)
  - [Tags](#tags)
  - [Keywords](#keywords)
  - [Test data files](#test-data-files)
- [Updating the documentation](#updating-the-documentation)
- [Running the unit tests](#running-the-unit-tests)
- [Self-review checklist](#self-review-checklist)

---

## IDE setup

For IDE setup, we recommend [PyCharm](https://www.jetbrains.com/fr-fr/pycharm/download) with the
[Robot Framework Language Server](https://plugins.jetbrains.com/plugin/16086-robot-framework-language-server) plugin.

After installation, define the working directory variable in Settings > Languages & Frameworks > Robot Framework
(Project): `{"EXECDIR": "{path}/ngsi-ld-test-suite"}`.

Two run configurations are pre-configured in `.idea/runConfigurations/`:

- `Check Format` — verifies that `.robot` / `.resource` files comply with Robocop formatting rules
- `Format Files` — applies Robocop formatting in place

---

## Commit messages

This project uses **[Conventional Commits](https://www.conventionalcommits.org/)**.

```
<type>(<optional scope>): <short description>
```

| Type       | When to use                                                 |
|------------|-------------------------------------------------------------|
| `feat`     | A new Test Case or a new feature (keyword, utility, script) |
| `fix`      | A correction to an existing Test Case or keyword            |
| `chore`    | Tooling, configuration, dependencies, CI/CD changes         |
| `refactor` | Internal restructuring with no behaviour change             |
| `docs`     | Documentation-only changes                                  |
| `test`     | Changes to the unit test suite under `doc/tests/`           |

The scope (in parentheses) is optional but encouraged when the change is limited to a specific API group or
subsystem, e.g. `feat(subscription)`, `fix(temporal)`, `chore(robocop)`.

The short description must be written in the **imperative mood** and must **not** end with a period.

```
# Good
feat(pick-omit): add TCs for Query Entities
fix(subscription): change default notification endpoint

# Bad
feat: Added some tests.
chore: various fixes
```

---

## Pre-commit hook

All `.robot` and `.resource` files are automatically formatted by [Robocop](https://robocop.readthedocs.io/) before each commit.

Install it once after cloning:

```bash
pip install pre-commit
pre-commit install
```

If the hook modifies files, the commit is rejected. Stage the reformatted files and commit again. To run the formatter
manually at any time:

```bash
robocop format
```

The active formatting rules are defined in `pyproject.toml` under `[tool.robocop.format]`. Do not disable or override
them locally.

---

## Authoring a new Test Case

### File and naming conventions

Test Cases are Robot Framework files located under `TP/NGSI-LD/`. Each file covers a single operation or group of
related operations and is named after the ETSI test group identifier:

```
TP/NGSI-LD/<Group>/<Subgroup>/<Operation>/<TC_ID>.robot
```

Test Case identifiers inside the file follow the pattern `XXX_YY_NN`, where:

- `XXX` is the numeric group identifier (e.g., `032`)
- `YY` is the subgroup index (e.g., `01`)
- `NN` is the sequential permutation number within the Test Case (e.g., `01`, `02`, …)

Example: `032_01_01 Delete Subscription By Id`.

### Test Case template

Use the following template as a starting point:

```robot
*** Settings ***
Documentation       {Describe the behaviour that is being checked by this Test Case}

Resource            ${EXECDIR}/resources/any/necessary/resource/file

Suite Setup         {Optional keyword to create data required by the Test Case}
Suite Teardown      {Optional keyword to delete data created in the setup step}
Test Template       {Keyword called for each permutation, if applicable}


*** Variables ***
${MY_VARIABLE}=     my_value


*** Test Cases ***    PARAM_1    PARAM_2
XXX_YY_01 Purpose of the first permutation
    [Tags]    {resource_request_reference}    {section_reference_1}
    param_1_value    param_2_value
XXX_YY_02 Purpose of the second permutation
    [Tags]    {resource_request_reference}    {section_reference_1}
    param_1_value    param_2_value


*** Keywords ***
{Keyword describing what the Test Case does}
    [Arguments]    ${param_1}    ${param_2}

    # 1. Call the operation under test
    # 2. Assert the response
    # 3. Optionally call a second endpoint for indirect verification
    #    (e.g. confirm an entity was actually created)
```

Setup keywords create data required by the Test Case and expose variables via `Set Suite Variable` or 
`Set Test Variable`. Teardown keywords **must** delete all data created during setup to leave the broker in a clean state.

### Tags

Every Test Case permutation **must** carry at least two tags:

- A `{resource_request_reference}` as defined in DGR/CIM-0015v211, section 6.1.
- One or more `{section_reference}` tags as defined in DGR/CIM-0015v211, section 6.2.

A single permutation may reference multiple sections when it exercises more than one part of the specification
(e.g. a Test Case that verifies scope creation during entity creation should tag both section 4.18 and 5.6.1).

### Keywords

- Reuse existing global keywords from `resources/` before writing new ones.
- New **assertion** keywords must be declared in `doc/analysis/checks.py` (in `self.checks` and `self.args`) and 
must include a method that generates the corresponding documentation string.
- New **endpoint** keywords must be declared in `doc/analysis/requests.py` (in `self.op` and `self.description`) and
must include the corresponding documentation method.
- Setup/Teardown keywords should preferably reuse keyword names already declared in `doc/analysis/initial_setup.py`.

### Test data files

Test data files (`.jsonld`, `.json`) live under `data/`. Place a new file in the subdirectory that best matches the
entity or operation it represents.

After adding or removing test data files, run the unused-file check to keep the directory clean:

```bash
python3 scripts/find_unused_test_data.py
```

---

## Updating the documentation

The test suite ships with an auto-generated documentation system. Changes that affect the documentation must be
accompanied by the appropriate updates.

**When a new Test Case is created:**

1. Declare it in `doc/tests/test_{group}.py`.
2. Run the documentation generator for the new TC:
   ```bash
   python doc/generateDocumentationData.py {new_tc_id}
   ```
3. Copy the generated file to the correct group folder:
   ```bash
   cp doc/results/{new_tc_id}.json doc/files/{group}/{subgroup}/
   ```
4. Update `doc/analysis/initial_setup.py` if a new setup keyword was introduced.
5. Update `doc/analysis/requests.py` if a new request parameter or endpoint was added.

**When a new permutation is added to an existing Test Case:**

Run the documentation generator for the affected TC and copy the output, as above.

**When a new directory of Test Cases is created:**

Declare it in `doc/analysis/generaterobotdata.py` along with its acronym.

### Generate documentation for support keywords

To generate documentation for the support keywords, for example, for Context Information Consumption resources, run
the following command:

```$ python3  -m robot.libdoc  resources/ApiUtils/ContextInformationConsumption.resource  api_docs/ContextInformationConsumption.html```

And, to generate documentation for the Test Cases:

```$ python3  -m robot.testdoc  TP/NGSI-LD  api_docs/TestCases.html```

---

## Running the unit tests

Before submitting, verify that the documentation unit tests pass:

```bash
python -m unittest discover -s ./doc/tests -t ./doc
```

These tests detect undeclared Test Cases and inconsistencies between the test suite files and the documentation
metadata. A failing unit test indicates either a missing declaration or a stale JSON documentation file.

---

## Maintenance scripts

### Find unused test data files

The `find_unused_test_data.py` script in the `scripts` directory can be used to get a list of the test data files
that are not used by any Test Case:

```
$ python3 scripts/find_unused_test_data.py
```

### Find and run Test Cases using a given test data file

The `find_tc_using_test_data.py` script in the `scripts` directory can be used to find the Test Cases that are using a 
given test data file. It can optionally run all the matching Test Cases:

```
$ python3 scripts/find_tc_using_test_data.py
```

When launched, the script asks for a test date file name and if the matching Test Cases should be executed.

### Find the list of defined Variables and Keywords in all resources files

The `apiutils.py` script in the `scripts` directory can be used to find the Variables and Keywords that are defined
in all resources files defined in the folder `resources/ApiUtils`.

```
$ python3 scripts/apiutils.py
```

---

## Self-review checklist

Before opening a merge request, confirm the following:

- [ ] Commit messages follow the Conventional Commits format.
- [ ] The pre-commit hook ran without errors (or reformatted files were re-staged).
- [ ] New Test Cases follow the naming convention (`XXX_YY_NN`) and carry the required tags.
- [ ] New or modified keywords are declared in the relevant `doc/analysis/` files.
- [ ] Documentation data has been regenerated for any new or modified Test Case.
- [ ] New test data files have been placed in the correct `data/` subdirectory.
- [ ] No unused test data files were introduced (`find_unused_test_data.py` returns no new entries).
- [ ] `python -m unittest discover -s ./doc/tests -t ./doc` passes locally.
