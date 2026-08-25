*** Settings ***
Documentation       Verify 5.14.2: Update EntityMap.
...
...                 5.14.2.4: partial update on the target EntityMap;
...                 provided output-only fields (entityMap, linkedMaps —
...                 5.2.39) shall be ignored. Unknown id → 404, invalid-URI
...                 id → 400. Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/entityMap.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections

Test Setup          Create Fixture EntityMap
Test Teardown       Delete Fixture EntityMap


*** Variables ***
${entity_id}=       urn:ngsi-ld:Vehicle:em5142


*** Test Cases ***
5142_01_01 Update ExpiresAt And Ignore Output-Only Members
    [Documentation]    5.14.2.4/.5: 204 on success; expiresAt applied, and per
    ...    Table 6.4.3.2-1 the actual expiry is set by the Context Broker,
    ...    "possibly overriding the requested duration";
    ...    a client-supplied entityMap member must NOT overwrite the
    ...    system-generated mapping.
    [Tags]    em-update    5_14_2    since_v1.9.1

    ${fragment}=    Set Variable
    ...    {"expiresAt": "2099-01-01T00:00:00Z", "entityMap": {"urn:ngsi-ld:Vehicle:injected": ["@none"]}}
    ${response}=    Update EntityMap    ${entityMapId}    ${fragment}
    Check Response Status Code    204    ${response.status_code}

    ${response}=    Retrieve EntityMap    ${entityMapId}
    Check Response Status Code    200    ${response.status_code}
    Should Be True    "${response.json()['expiresAt']}" < "2099-01-01T00:00:00Z"
    Dictionary Should Not Contain Key
    ...    ${response.json()['entityMap']}
    ...    urn:ngsi-ld:Vehicle:injected
    Dictionary Should Contain Key    ${response.json()['entityMap']}    ${entity_id}

5142_01_02 Update With An Invalid ExpiresAt
    [Documentation]    4.6.3: expiresAt must be a DateTime → 400.
    [Tags]    em-update    5_14_2    since_v1.9.1

    ${response}=    Update EntityMap    ${entityMapId}    {"expiresAt": "tomorrow"}
    Check Response Status Code    400    ${response.status_code}

5142_01_03 Update An Unknown EntityMap
    [Documentation]    5.14.2.4: no matching EntityMap → ResourceNotFound.
    [Tags]    em-update    5_14_2    since_v1.9.1

    ${response}=    Update EntityMap
    ...    urn:ngsi-ld:entitymap:doesnotexist
    ...    {"expiresAt": "2099-01-01T00:00:00Z"}
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
