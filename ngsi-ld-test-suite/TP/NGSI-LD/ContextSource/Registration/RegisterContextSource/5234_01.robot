*** Settings ***
Documentation       Check the RegistrationManagementInfo data type (CIM 009 clause 5.2.34,
...                 Table 5.2.34-1) on a Context Source Registration's management member:
...                 cacheDuration is an ISO 8601 duration, cooldown and timeout are
...                 numbers greater than 0, localOnly is a boolean.
...
...                 Antares extension TP — the official TPs never send the management
...                 member.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
5234_01_01 A Conformant Management Object Is Accepted
    [Documentation]    control: Table 5.2.34-1-conformant management stays registrable
    [Tags]    csr-create    5_2_34    since_v1.9.1
    Register With Management Expecting    201
    ...    {"cacheDuration": "PT5M", "cooldown": 500, "timeout": 3000, "localOnly": true}

5234_01_02 A Non-Object Management Member Is Rejected
    [Tags]    csr-create    5_2_34    since_v1.9.1
    Register With Management Expecting    400    "yes"

5234_01_03 An Invalid CacheDuration Is Rejected
    [Documentation]    Table 5.2.34-1: cacheDuration — ISO 8601 duration
    [Tags]    csr-create    5_2_34    since_v1.9.1
    Register With Management Expecting    400    {"cacheDuration": "bogus"}

5234_01_04 A Zero Cooldown Is Rejected
    [Documentation]    Table 5.2.34-1: cooldown — "Greater than 0"
    [Tags]    csr-create    5_2_34    since_v1.9.1
    Register With Management Expecting    400    {"cooldown": 0}

5234_01_05 A Negative Timeout Is Rejected
    [Documentation]    Table 5.2.34-1: timeout — "Greater than 0"
    [Tags]    csr-create    5_2_34    since_v1.9.1
    Register With Management Expecting    400    {"timeout": -5}

5234_01_06 A Non-Boolean LocalOnly Is Rejected
    [Tags]    csr-create    5_2_34    since_v1.9.1
    Register With Management Expecting    400    {"localOnly": "yes"}


*** Keywords ***
Register With Management Expecting
    [Arguments]    ${expected_status_code}    ${management_json}
    ${id}=    Generate Random CSR Id
    ${body}=    Evaluate
    ...    json.dumps({"id": $id, "type": "ContextSourceRegistration", "endpoint": "http://peer.example/ngsi-ld/v1", "information": [{"entities": [{"type": "Building"}]}], "management": json.loads('''${management_json}''')})
    ...    modules=json
    ${response}=    POST
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}
    ...    data=${body}
    ...    headers=${{ {"Content-Type": "application/json"} }}
    ...    expected_status=any
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    ${expected_status_code} == 201
        Delete Context Source Registration    ${id}
    END
