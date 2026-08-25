*** Settings ***
Documentation       Verify the EntityMap tenancy and lifetime edges the
...                 5141_01/5142_01/5144_01 TPs do not reach.
...
...                 4.14: with tenants "an NGSI-LD system shall behave as if
...                 the tenants were separate systems", so an EntityMap
...                 created under one tenant is unknown to another —
...                 retrieve and delete both answer 404 ResourceNotFound and
...                 the owner's map survives. 5.5.14: an expired Entity Map
...                 "cannot be accessed"; Table 6.4.3.2-1: "the actual
...                 expiresAt time of the EntityMap shall be set by the
...                 Context Broker or Context Source, possibly overriding the
...                 requested duration". Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/entityMap.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections
Library             DateTime

Test Setup          Create Fixture EntityMap
Test Teardown       Delete Fixture EntityMap


*** Variables ***
${entity_id}=       urn:ngsi-ld:Vehicle:em5141b
${other_tenant}=    em5141bothertenant


*** Test Cases ***
5141_02_01 An EntityMap Is Not Visible To Another Tenant
    [Documentation]    4.14: another tenant must not read the map — 404
    ...    ResourceNotFound, never the 5.2.39 object.
    [Tags]    em-retrieve    5_14_1    4_14    since_v1.9.1

    &{headers}=    Create Dictionary    NGSILD-Tenant=${other_tenant}
    ${quoted}=    Evaluate    urllib.parse.quote("${entityMapId}", safe='')    modules=urllib
    ${response}=    GET
    ...    url=${url}/entityMaps/${quoted}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    404    ${response.status_code}
    Should Not Contain    ${response.text}    ${entity_id}

5141_02_02 Another Tenant Cannot Delete The EntityMap
    [Documentation]    4.14: a cross-tenant delete is a 404 and leaves the
    ...    owner's EntityMap intact.
    [Tags]    em-delete    5_14_3    4_14    since_v1.9.1

    &{headers}=    Create Dictionary    NGSILD-Tenant=${other_tenant}
    ${quoted}=    Evaluate    urllib.parse.quote("${entityMapId}", safe='')    modules=urllib
    ${response}=    DELETE
    ...    url=${url}/entityMaps/${quoted}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    404    ${response.status_code}

    ${response}=    Retrieve EntityMap    ${entityMapId}
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()['entityMap']}    ${entity_id}

5141_02_03 The Broker Bounds The Requested Lifetime
    [Documentation]    Table 6.4.3.2-1: the broker sets the actual expiresAt,
    ...    "possibly overriding the requested duration" — an extreme
    ...    entityMapLifetime does not become the map's expiry verbatim.
    [Tags]    em-create    5_14_4    since_v1.9.1

    ${response}=    Create EntityMap For Query    type=Vehicle    entityMapLifetime=P3650D
    Check Response Status Code    201    ${response.status_code}
    ${expiry}=    Convert Date    ${response.json()['expiresAt']}    result_format=epoch
    ${now}=    Get Current Date    time_zone=UTC    result_format=epoch
    ${ahead}=    Evaluate    ${expiry} - ${now}
    Should Be True    ${ahead} > 0    an EntityMap is created live
    Should Be True    ${ahead} < 315360000    a ten-year lifetime is not honoured verbatim
    ${mapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Delete EntityMap    ${mapId}

5141_02_04 An Expired EntityMap Cannot Be Accessed
    [Documentation]    5.5.14: "an expired Entity Map cannot be accessed" —
    ...    once its expiresAt is in the past the map is 404, not served.
    [Tags]    em-retrieve    5_14_1    5_5_14    since_v1.9.1

    ${response}=    Create EntityMap For Query    type=Vehicle
    Check Response Status Code    201    ${response.status_code}
    ${mapId}=    Fetch EntityMap Id From Response    ${response.headers}
    ${response}=    Update EntityMap    ${mapId}    {"expiresAt": "2000-01-01T00:00:00Z"}
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve EntityMap    ${mapId}
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
