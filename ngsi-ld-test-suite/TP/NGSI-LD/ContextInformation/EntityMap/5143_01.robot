*** Settings ***
Documentation       Verify 5.14.3: Delete EntityMap.
...
...                 5.14.3.4: the EntityMap is removed from the broker's
...                 storage/memory; invalid-URI id → 400 BadRequestData,
...                 unknown id → 404 ResourceNotFound. Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/entityMap.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections

Test Setup          Create Fixture EntityMap
Test Teardown       Delete Fixture Entity


*** Variables ***
${entity_id}=       urn:ngsi-ld:Vehicle:em5143


*** Test Cases ***
5143_01_01 Delete An EntityMap
    [Documentation]    5.14.3.4/.5: 204 with no content; the map is gone
    ...    afterwards (404 on retrieve AND on a second delete).
    [Tags]    em-delete    5_14_3    since_v1.9.1

    ${response}=    Delete EntityMap    ${entityMapId}
    Check Response Status Code    204    ${response.status_code}
    Should Be Empty    ${response.text}

    ${response}=    Retrieve EntityMap    ${entityMapId}
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Delete EntityMap    ${entityMapId}
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}

5143_01_02 Delete With An Invalid URI Id
    [Documentation]    5.14.3.4: "not a valid URI" → BadRequestData.
    [Tags]    em-delete    5_14_3    since_v1.9.1

    ${response}=    Delete EntityMap    not a uri
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Delete EntityMap    ${entityMapId}


*** Keywords ***
Create Fixture EntityMap
    ${response}=    Create EntityMap Test Entity    ${entity_id}    50
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create EntityMap For Query    type=Vehicle
    Check Response Status Code    201    ${response.status_code}
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Set Suite Variable    ${entityMapId}

Delete Fixture Entity
    Delete EntityMap Test Entity    ${entity_id}
