*** Settings ***
Documentation       Verify 5.14.4: Create EntityMap for Query Entities.
...
...                 5.14.4.4: the broker runs the query and returns an
...                 EntityMap listing the matching Entity ids; local entities
...                 are marked "@none" (5.2.39). Too-wide queries (no type/
...                 attrs/q/georel/local) are 400 BadRequestData. 6.34.3.1:
...                 201 Created + NGSILD-EntityMap header with the resource
...                 URI. Antares extension TP — no official 5.14 coverage.

Resource            ${EXECDIR}/resources/ApiUtils/entityMap.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections

Test Setup          Create Fixture Entities
Test Teardown       Delete Fixture Entities


*** Variables ***
${slow_vehicle}=    urn:ngsi-ld:Vehicle:em5144slow
${fast_vehicle}=    urn:ngsi-ld:Vehicle:em5144fast


*** Test Cases ***
5144_01_01 Create EntityMap For A Type Query
    [Documentation]    5.14.4.4/.5: matching ids land in entityMap under
    ...    "@none"; 201 + NGSILD-EntityMap header (6.34.3.1).
    [Tags]    em-create    5_14_4    since_v1.9.1

    ${response}=    Create EntityMap For Query    type=Vehicle
    Check Response Status Code    201    ${response.status_code}
    Dictionary Should Contain Key    ${response.headers}    NGSILD-EntityMap
    Should Be Equal    ${response.json()['type']}    EntityMap
    Should Not Be Empty    ${response.json()['expiresAt']}
    ${sources}=    Get From Dictionary    ${response.json()['entityMap']}    ${slow_vehicle}
    Should Be Equal As Strings    ${sources}    ['@none']
    Dictionary Should Contain Key    ${response.json()['entityMap']}    ${fast_vehicle}
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Delete EntityMap    ${entityMapId}

5144_01_02 Query Filters Narrow The EntityMap
    [Documentation]    5.14.4.4: the q filter applies — only matching
    ...    Entities are candidates; the non-matching id must NOT appear.
    [Tags]    em-create    5_14_4    since_v1.9.1

    ${response}=    Create EntityMap For Query    type=Vehicle    q=speed>50
    Check Response Status Code    201    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()['entityMap']}    ${fast_vehicle}
    Dictionary Should Not Contain Key    ${response.json()['entityMap']}    ${slow_vehicle}
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Delete EntityMap    ${entityMapId}

5144_01_03 Too Wide Query Is Rejected
    [Documentation]    5.14.4.4: no type/attrs/q/georel/local → 400
    ...    BadRequestData (too wide query).
    [Tags]    em-create    5_14_4    since_v1.9.1

    ${response}=    Create EntityMap For Query    id=${slow_vehicle}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

5144_01_04 Invalid EntityMapLifetime Is Rejected
    [Documentation]    Table 6.4.3.2-1: entityMapLifetime is an ISO 8601
    ...    duration; anything else is 400.
    [Tags]    em-create    5_14_4    since_v1.9.1

    ${response}=    Create EntityMap For Query    type=Vehicle    entityMapLifetime=1hour
    Check Response Status Code    400    ${response.status_code}

5144_01_05 EntityMap Requested On The Entity Query
    [Documentation]    6.4.3.2: entityMap=true on GET /entities → 201 +
    ...    NGSILD-EntityMap header, entity payload unchanged; the referenced
    ...    map then fixes a follow-up query (5.5.14) and echoes the header.
    [Tags]    em-create    5_14_4    5_5_14    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/entities
    ...    params=type=Vehicle&entityMap=true
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    Dictionary Should Contain Key    ${response.headers}    NGSILD-EntityMap
    ${ids}=    Evaluate    sorted(e["id"] for e in $response.json())
    Should Be Equal As Strings    ${ids}    ['${fast_vehicle}', '${slow_vehicle}']
    ${maploc}=    Get From Dictionary    ${response.headers}    NGSILD-EntityMap

    &{headers}=    Create Dictionary    NGSILD-EntityMap=${maploc}
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=type=Vehicle
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${echoed}=    Get From Dictionary    ${response.headers}    NGSILD-EntityMap
    Should Be Equal    ${echoed}    ${maploc}
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Delete EntityMap    ${entityMapId}


*** Keywords ***
Create Fixture Entities
    ${response}=    Create EntityMap Test Entity    ${slow_vehicle}    30
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create EntityMap Test Entity    ${fast_vehicle}    90
    Check Response Status Code    201    ${response.status_code}

Delete Fixture Entities
    Delete EntityMap Test Entity    ${slow_vehicle}
    Delete EntityMap Test Entity    ${fast_vehicle}
