*** Settings ***
Documentation       Verify that a broker deployed WITHOUT a temporal store answers
...                 temporal API requests with the error type OperationNotSupported and
...                 HTTP 422, per Table 6.3.2-1
...                 (https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported -> 422),
...                 instead of 404s, empty results, or a 5xx — and that the current-state
...                 API keeps working untouched.
...
...                 Antares extension TP. Requires a broker started with
...                 ANTARES_TEMPORAL=none — tagged config_no_temporal and excluded from
...                 the default conformance runs, which assume a temporal store.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

*** Variables ***
${ons_type}=    https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported


*** Test Cases ***
632_01_01 Query Temporal Evolution Is Refused With OperationNotSupported
    [Tags]    common-behaviours    6_3_2    config_no_temporal    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/temporal/entities/
    ...    params=type=Building&timerel=after&timeAt=2020-08-01T12:00:00Z
    ...    expected_status=any
    Refused As Unsupported    ${response}

632_01_02 Retrieve Temporal Evolution Is Refused With OperationNotSupported
    [Tags]    common-behaviours    6_3_2    config_no_temporal    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/temporal/entities/urn:ngsi-ld:Building:632-01
    ...    expected_status=any
    Refused As Unsupported    ${response}

632_01_03 Current State Operations Are Untouched
    [Documentation]    the missing temporal store degrades ONLY the temporal surface:
    ...    an entity create and read must behave exactly as with history enabled
    [Tags]    common-behaviours    6_3_2    config_no_temporal    since_v1.9.1

    ${body}=    Set Variable    {"id": "urn:ngsi-ld:Building:632-01", "type": "Building", "name": {"type": "Property", "value": "x"}}
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    data=${body}
    ...    headers=${{ {"Content-Type": "application/json"} }}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}urn:ngsi-ld:Building:632-01
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()}[id]    urn:ngsi-ld:Building:632-01
    [Teardown]    DELETE    url=${url}/${ENTITIES_ENDPOINT_PATH}urn:ngsi-ld:Building:632-01    expected_status=any


*** Keywords ***
Refused As Unsupported
    [Arguments]    ${response}
    Check Response Status Code    422    ${response.status_code}
    Should Be Equal    ${response.json()}[type]    ${ons_type}
    Should Not Be Equal As Integers    ${response.status_code}    404
    Should Not Be Equal As Integers    ${response.status_code}    500
