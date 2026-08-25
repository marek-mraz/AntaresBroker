*** Settings ***
Documentation       Verify 5.14.1: Retrieve EntityMap.
...
...                 5.14.1.4: an id that is not a valid URI → 400
...                 BadRequestData; an unknown EntityMap id → 404
...                 ResourceNotFound; otherwise the 5.2.39 JSON-LD object
...                 (id, type "EntityMap", expiresAt, entityMap, linkedMaps).
...                 Antares extension TP — no official 5.14 coverage.

Resource            ${EXECDIR}/resources/ApiUtils/entityMap.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections

Test Setup          Create Fixture EntityMap
Test Teardown       Delete Fixture EntityMap


*** Variables ***
${entity_id}=       urn:ngsi-ld:Vehicle:em5141


*** Test Cases ***
5141_01_01 Retrieve An Existing EntityMap
    [Documentation]    5.14.1.4/.5: the target EntityMap comes back as
    ...    mandated by 5.2.39 — and must NOT carry entity payload members.
    [Tags]    em-retrieve    5_14_1    since_v1.9.1

    ${response}=    Retrieve EntityMap    ${entityMapId}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['type']}    EntityMap
    Dictionary Should Contain Key    ${response.json()['entityMap']}    ${entity_id}
    Dictionary Should Contain Key    ${response.json()}    expiresAt
    Dictionary Should Not Contain Key    ${response.json()}    speed

5141_01_02 Retrieve With An Invalid URI Id
    [Documentation]    5.14.1.4: "not a valid URI" → BadRequestData.
    [Tags]    em-retrieve    5_14_1    since_v1.9.1

    ${response}=    Retrieve EntityMap    not a uri
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

5141_01_03 Retrieve An Unknown EntityMap
    [Documentation]    5.14.1.4: no matching EntityMap → ResourceNotFound.
    [Tags]    em-retrieve    5_14_1    since_v1.9.1

    ${response}=    Retrieve EntityMap    urn:ngsi-ld:entitymap:doesnotexist
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}


*** Keywords ***
Create Fixture EntityMap
    ${response}=    Create EntityMap Test Entity    ${entity_id}    50
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create EntityMap For Query    type=Vehicle
    Check Response Status Code    201    ${response.status_code}
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Set Suite Variable    ${entityMapId}

Delete Fixture EntityMap
    Delete EntityMap    ${entityMapId}
    Delete EntityMap Test Entity    ${entity_id}
