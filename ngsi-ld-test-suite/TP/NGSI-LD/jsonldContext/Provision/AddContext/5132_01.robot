*** Settings ***
Documentation       Verify 5.13.2.4 JSON-LD validation of the supplied @context.
...
...                 5.13.2.4: "The behaviour described in clause 5.5.4 about
...                 JSON and JSON-LD validation shall be applied in case of
...                 invalid @context." A top-level @context value that is not
...                 a string, an object, or an array of those is invalid
...                 JSON-LD and shall be rejected with 400 BadRequestData.
...
...                 Antares extension TP — 050_02 covers a missing @context
...                 member and malformed JSON; this covers well-formed JSON
...                 whose @context VALUE is not a JSON-LD local context.

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections

Test Template       Add an invalid @context


*** Test Cases ***    FILENAME
5132_01_01 Number As @context Value
    [Tags]    ctx-add    5_13_2    since_v1.9.1
    @context-invalid-number.json
5132_01_02 Boolean As @context Value
    [Tags]    ctx-add    5_13_2    since_v1.9.1
    @context-invalid-boolean.json
5132_01_03 Array With A Number Item As @context Value
    [Tags]    ctx-add    5_13_2    since_v1.9.1
    @context-invalid-array-item.json


*** Keywords ***
Add an invalid @context
    [Documentation]    5.13.2.4 + 5.5.4: invalid JSON-LD @context → 400
    ...    BadRequestData, and no Location header (nothing stored).
    [Arguments]    ${filename}

    ${response}=    Add a new @context    ${filename}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Dictionary Should Not Contain Key    ${response.headers}    Location
