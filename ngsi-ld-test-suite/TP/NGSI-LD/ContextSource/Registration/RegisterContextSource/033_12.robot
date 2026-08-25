*** Settings ***
Documentation       Check the KeyValuePair data type (CIM 009 clause 5.2.22, Table 5.2.22-1)
...                 on a Context Source Registration's contextSourceInfo (clause 5.2.9):
...                 both key and value are Strings with cardinality 1 — a pair with a
...                 non-string or missing key/value is BadRequestData at registration time.
...
...                 Antares extension TP — the official TPs only exercise string pairs.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
033_12_01 String Key And Value Pairs Are Accepted
    [Documentation]    control: Table 5.2.22-1-conforming pairs stay registrable
    [Tags]    csr-create    5_2_22    since_v1.9.1
    Register With ContextSourceInfo Expecting    201    [{"key": "X-Auth-Token", "value": "abc"}]

033_12_02 A Numeric Value Is Rejected
    [Tags]    csr-create    5_2_22    since_v1.9.1
    Register With ContextSourceInfo Expecting    400    [{"key": "X-Custom", "value": 123}]

033_12_03 An Array Value Is Rejected
    [Tags]    csr-create    5_2_22    since_v1.9.1
    Register With ContextSourceInfo Expecting    400    [{"key": "X-Custom", "value": ["a"]}]

033_12_04 A Pair Without Value Is Rejected
    [Documentation]    value cardinality is 1
    [Tags]    csr-create    5_2_22    since_v1.9.1
    Register With ContextSourceInfo Expecting    400    [{"key": "X-Custom"}]

033_12_05 A Numeric Key Is Rejected
    [Tags]    csr-create    5_2_22    since_v1.9.1
    Register With ContextSourceInfo Expecting    400    [{"key": 5, "value": "v"}]


*** Keywords ***
Register With ContextSourceInfo Expecting
    [Arguments]    ${expected_status_code}    ${context_source_info}
    ${id}=    Generate Random CSR Id
    ${body}=    Evaluate
    ...    json.dumps({"id": $id, "type": "ContextSourceRegistration", "endpoint": "http://peer.example/ngsi-ld/v1", "information": [{"entities": [{"type": "Building"}]}], "contextSourceInfo": json.loads($context_source_info)})
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
