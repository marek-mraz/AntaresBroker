*** Settings ***
Documentation       Verify 5.7.1.4 EntityMap usage on Retrieve Entity.
...
...                 5.7.1.4: "If a flag to return an EntityMap was present
...                 in the request, and no EntityMap currently exists, then
...                 a new EntityMap shall be created"; "If the resource
...                 cannot be found, or the data has expired, a new
...                 EntityMap shall be created"; a live map's location is
...                 returned in the NGSILD-EntityMap header.
...                 Antares extension TP — no official EntityMap coverage.

Resource            ${EXECDIR}/resources/ApiUtils/entityMap.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections

Test Setup          Create Fixture Entity
Test Teardown       Delete Fixture Entity


*** Variables ***
${vehicle}=     urn:ngsi-ld:Vehicle:em5714


*** Test Cases ***
5714_01_01 Retrieve With EntityMap Flag Creates A Map
    [Documentation]    5.7.1.4: entityMap=true on GET /entities/{id} →
    ...    a new map holding the entity under "@none" (5.2.39); the
    ...    retrieve body stays an Entity (no entityMap member).
    [Tags]    em-usage    5_7_1_4    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/entities/${vehicle}
    ...    params=entityMap=true
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.headers}    NGSILD-EntityMap
    Should Be Equal    ${response.json()['id']}    ${vehicle}
    Dictionary Should Not Contain Key    ${response.json()}    entityMap
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    ${map}=    Retrieve EntityMap    ${entityMapId}
    Check Response Status Code    200    ${map.status_code}
    ${sources}=    Get From Dictionary    ${map.json()['entityMap']}    ${vehicle}
    Should Be Equal As Strings    ${sources}    ['@none']
    Delete EntityMap    ${entityMapId}

5714_01_02 Unknown EntityMap Reference Creates A New Map
    [Documentation]    5.7.1.4: "If the resource cannot be found, or the
    ...    data has expired, a new EntityMap shall be created" — no error,
    ...    and the dead reference must NOT be echoed back.
    [Tags]    em-usage    5_7_1_4    since_v1.9.1

    &{headers}=    Create Dictionary    NGSILD-EntityMap=urn:ngsi-ld:entitymap:em5714dead
    ${response}=    GET
    ...    url=${url}/entities/${vehicle}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.headers}    NGSILD-EntityMap
    ${loc}=    Get From Dictionary    ${response.headers}    NGSILD-EntityMap
    Should Not Contain    ${loc}    em5714dead
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Delete EntityMap    ${entityMapId}

5714_01_03 Live EntityMap Location Is Echoed
    [Documentation]    5.7.1.4: a live referenced map is used and its
    ...    location returned unchanged in NGSILD-EntityMap.
    [Tags]    em-usage    5_7_1_4    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/entities/${vehicle}
    ...    params=entityMap=true
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${maploc}=    Get From Dictionary    ${response.headers}    NGSILD-EntityMap

    &{headers}=    Create Dictionary    NGSILD-EntityMap=${maploc}
    ${response}=    GET
    ...    url=${url}/entities/${vehicle}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${echoed}=    Get From Dictionary    ${response.headers}    NGSILD-EntityMap
    Should Be Equal    ${echoed}    ${maploc}
    Should Be Equal    ${response.json()['id']}    ${vehicle}
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Delete EntityMap    ${entityMapId}


*** Keywords ***
Create Fixture Entity
    ${response}=    Create EntityMap Test Entity    ${vehicle}    30
    Check Response Status Code    201    ${response.status_code}

Delete Fixture Entity
    Delete EntityMap Test Entity    ${vehicle}
