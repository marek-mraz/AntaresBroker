*** Settings ***
Documentation       Verify 5.14.5: Create EntityMap for Query Temporal
...                 Evolution of Entities.
...
...                 5.14.5.4: "If a temporal query is not provided then an
...                 error of type BadRequestData shall be raised"; with one,
...                 the Entities whose instances match the temporal window
...                 (S1-S4) become the EntityMap candidates. 6.35.3.1: 201 +
...                 NGSILD-EntityMap header. Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/entityMap.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections

Test Setup          Create Fixture Entity
Test Teardown       Delete Fixture Entity


*** Variables ***
${entity_id}=       urn:ngsi-ld:Vehicle:em5145


*** Test Cases ***
5145_01_01 Temporal EntityMap Requires A Temporal Query
    [Documentation]    5.14.5.4: no timerel/timeAt → 400 BadRequestData.
    [Tags]    em-create-temporal    5_14_5    since_v1.9.1

    ${response}=    Create Temporal EntityMap For Query    type=Vehicle
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

5145_01_02 Create A Temporal EntityMap
    [Documentation]    5.14.5.4/.5: entities with instances inside the window
    ...    are candidates ("@none"); an empty window yields an empty map —
    ...    the entity must NOT appear.
    [Tags]    em-create-temporal    5_14_5    since_v1.9.1

    ${response}=    Create Temporal EntityMap For Query
    ...    type=Vehicle
    ...    timerel=before
    ...    timeAt=2999-01-01T00:00:00Z
    ...    timeproperty=createdAt
    Check Response Status Code    201    ${response.status_code}
    Dictionary Should Contain Key    ${response.headers}    NGSILD-EntityMap
    Should Be Equal    ${response.json()['type']}    EntityMap
    Dictionary Should Contain Key    ${response.json()['entityMap']}    ${entity_id}
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Delete EntityMap    ${entityMapId}

    ${response}=    Create Temporal EntityMap For Query
    ...    type=Vehicle
    ...    timerel=before
    ...    timeAt=2000-01-01T00:00:00Z
    ...    timeproperty=createdAt
    Check Response Status Code    201    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.json()['entityMap']}    ${entity_id}
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Delete EntityMap    ${entityMapId}


*** Keywords ***
Create Fixture Entity
    ${response}=    Create EntityMap Test Entity    ${entity_id}    50
    Check Response Status Code    201    ${response.status_code}

Delete Fixture Entity
    Delete EntityMap Test Entity    ${entity_id}
    ${response}=    DELETE
    ...    url=${url}/temporal/entities/${entity_id}
    ...    expected_status=any
