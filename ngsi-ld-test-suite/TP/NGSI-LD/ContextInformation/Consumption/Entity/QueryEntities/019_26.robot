*** Settings ***
Documentation       Check the `type=*` wildcard on Query Entities.
...
...                 Table 6.4.3.2-1 (`type`): "Selection of Entity Types as per clause
...                 4.17. \"*\" is also allowed as a value and local is implicitly set to
...                 true and shall not be explicitly set to false."
...
...                 Antares extension TP — no official TP covers the wildcard. Regression
...                 guard: expanding "*" as an ordinary term yielded an IRI nothing
...                 matched, so `type=*` answered 200 with an empty array.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create Two Entities Of Different Types
Suite Teardown      Delete Both Entities


*** Test Cases ***
019_26_01 Query Entities With Type Wildcard Returns Every Type
    [Documentation]    "*" selects all Entity Types, not a type literally named "*"
    [Tags]    e-query    4_17    6_4_3_2    since_v1.9.1

    ${response}=    Query Entities    entity_types=*    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    ${ids}=    Evaluate    [e.get("id") for e in $response.json()]
    Should Contain    ${ids}    ${building_id}    the wildcard must return the Building
    Should Contain    ${ids}    ${vehicle_id}    the wildcard must return the Vehicle too

019_26_02 Query Entities With Type Wildcard And Explicit Local False
    [Documentation]    "…and shall not be explicitly set to false" — the combination is
    ...    contradictory and must be rejected
    [Tags]    e-query    4_17    6_4_3_2    since_v1.9.1

    ${response}=    Query Entities
    ...    entity_types=*
    ...    local=false
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Create Two Entities Of Different Types
    ${building_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${building_id}
    ${response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${building_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

    ${vehicle_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${vehicle_id}
    ${vehicle}=    Evaluate
    ...    {"id": $vehicle_id, "type": "Vehicle", "brandName": {"type": "Property", "value": "Mercedes"}, "@context": $ngsild_test_suite_context}
    ${response}=    Create Entity From JSON-LD Content    ${vehicle}
    Check Response Status Code    201    ${response.status_code}

Delete Both Entities
    Delete Entity    ${building_id}
    Delete Entity    ${vehicle_id}
