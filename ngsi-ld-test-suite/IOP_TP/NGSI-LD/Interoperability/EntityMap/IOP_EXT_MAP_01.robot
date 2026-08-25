*** Settings ***
Documentation       Distributed EntityMaps (Antares extension IOP TPs).
...                 5.2.39: entityMap is "a set of key-value pairs whose
...                 keys shall be strings representing Entity ids and whose
...                 values shall be an array holding every
...                 CSourceRegistration id which is relevant ... The key
...                 '@none' shall be used to refer to an Entity that is
...                 held locally"; expiresAt is the one writable member —
...                 output-only members "shall ignore them" when provided.
...                 5.7.2.4: "If the resource cannot be found, or the data
...                 has expired, a new EntityMap shall be created; if not
...                 expired, only the retrieved EntityMap shall be used".
...                 5.5.9.3: "Entities not or no longer fitting the query
...                 shall be removed from the Entity map during
...                 pagination". 5.14.2.4 update / 5.14.3.4 delete.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections
Library             RequestsLibrary

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${b3_url}


*** Test Cases ***
IOP_EXT_MAP_01_01 The EntityMap Names Each Entity's Sources, @none For Local
    [Documentation]    5.2.39 Table 5.2.39-2: entityMap values hold "every
    ...    CSourceRegistration id which is relevant"; "The key '@none'
    ...    shall be used to refer to an Entity that is held locally."
    [Tags]    iop    iop-ext    5_2_39    4_3_6_7    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${l}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}

    ${created}=    Query Entities Via Broker    ${b1_url}    type=${etype}    entityMap=true
    Check Response Status Code    201    ${created.status_code}
    ${map_id}=    Map Id From    ${created.headers}
    ${map}=    GET    url=${b1_url}/entityMaps/${map_id}    expected_status=any
    Check Response Status Code    200    ${map.status_code}
    ${local_srcs}=    Evaluate    $map.json()['entityMap'][$entity_id + "-l"]
    Should Contain    ${local_srcs}    @none
    ${remote_srcs}=    Evaluate    $map.json()['entityMap'][$entity_id + "-r"]
    Should Contain    ${remote_srcs}    ${registration_id}
    Should Not Contain    ${remote_srcs}    @none

IOP_EXT_MAP_01_02 An Expired Map Reference Recreates A Fresh Map
    [Documentation]    5.7.2.4: "If the resource cannot be found, or the
    ...    data has expired, a new EntityMap shall be created" — a query
    ...    presenting an expired map still answers, under a NEW map id.
    [Tags]    iop    iop-ext    5_7_2    5_14_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}
    ${created}=    Query Entities Via Broker    ${b1_url}    type=${etype}    entityMap=true
    Check Response Status Code    201    ${created.status_code}
    ${map_ref}=    Get From Dictionary    ${created.headers}    NGSILD-EntityMap
    ${map_id}=    Map Id From    ${created.headers}
    ${expire}=    Evaluate    {"expiresAt": "2020-01-01T00:00:00Z"}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${patched}=    PATCH    url=${b1_url}/entityMaps/${map_id}    json=${expire}
    ...    headers=${headers}    expected_status=any
    Should Be True    ${patched.status_code} in (204, 207)

    ${reused}=    Query Entities Via Broker    ${b1_url}    map_ref=${map_ref}    type=${etype}
    Should Be True    ${reused.status_code} in (200, 201)
    Should Contain    ${reused.text}    ${entity_id}-r
    ${new_ref}=    Get From Dictionary    ${reused.headers}    NGSILD-EntityMap
    Should Not Be Equal    ${new_ref}    ${map_ref}

IOP_EXT_MAP_01_03 A Mid-Walk Remote Deletion Is Tolerated, Never Served
    [Documentation]    5.5.9.3: with Entity maps "filters shall be
    ...    rechecked before returning results ... Entities not or no longer
    ...    fitting the query shall be removed from the Entity map during
    ...    pagination" — the remotely deleted entity drops out without
    ...    failing the walk.
    [Tags]    iop    iop-ext    5_5_9_3    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${l}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}
    ${created}=    Query Entities Via Broker    ${b1_url}    type=${etype}    entityMap=true
    Check Response Status Code    201    ${created.status_code}
    ${map_ref}=    Get From Dictionary    ${created.headers}    NGSILD-EntityMap

    Delete Entity Via Broker    ${b2_url}    ${entity_id}-r

    ${walk}=    Query Entities Via Broker    ${b1_url}    map_ref=${map_ref}    type=${etype}
    Check Response Status Code    200    ${walk.status_code}
    Should Contain    ${walk.text}    ${entity_id}-l
    Should Not Contain    ${walk.text}    ${entity_id}-r

IOP_EXT_MAP_01_04 A Scoped Map Never Leaks Non-Matching Remote Ids
    [Documentation]    5.14.4.4/5.5.9.3: the map holds only candidates of
    ...    the creating query, and filters are rechecked per page — an
    ...    entity outside the q filter appears on no page.
    [Tags]    iop    iop-ext    5_14_4    5_5_9_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${hi}=    Simple Vehicle Entity    ${entity_id}-hi    ${etype}    500
    Create Entity At Broker    ${b2_url}    ${hi}
    ${hi2}=    Simple Vehicle Entity    ${entity_id}-hi2    ${etype}    600
    Create Entity At Broker    ${b1_url}    ${hi2}
    ${lo}=    Simple Vehicle Entity    ${entity_id}-lo    ${etype}    5
    Create Entity At Broker    ${b2_url}    ${lo}
    ${created}=    Query Entities Via Broker    ${b1_url}    type=${etype}    q=speed>100
    ...    entityMap=true
    Check Response Status Code    201    ${created.status_code}
    ${map_ref}=    Get From Dictionary    ${created.headers}    NGSILD-EntityMap

    ${page1}=    Query Entities Via Broker    ${b1_url}    map_ref=${map_ref}    type=${etype}
    ...    q=speed>100    limit=1
    Check Response Status Code    200    ${page1.status_code}
    ${page2}=    Query Entities Via Broker    ${b1_url}    map_ref=${map_ref}    type=${etype}
    ...    q=speed>100    limit=1    offset=1
    Check Response Status Code    200    ${page2.status_code}
    ${union}=    Evaluate    $page1.text + $page2.text
    Should Contain    ${union}    ${entity_id}-hi
    Should Contain    ${union}    ${entity_id}-hi2
    Should Not Contain    ${union}    ${entity_id}-lo

IOP_EXT_MAP_01_05 PATCH Applies expiresAt And Ignores Output-Only Members
    [Documentation]    5.14.2.4: "Perform an update operation on the target
    ...    EntityMap using the fields specified within the JSON-LD
    ...    document. Any provided output-only fields shall be ignored" —
    ...    expiresAt (Table 5.2.39-1) applies, though Table 6.4.3.2-1 lets
    ...    the broker set the actual expiry, "possibly overriding the
    ...    requested duration" — so the assertion is that the expiry CHANGED
    ...    toward the request and never past it, not byte equality with a
    ...    far-future instant a broker may legally clamp. entityMap
    ...    (Table 5.2.39-2, output-only) is ignored, never stored.
    [Tags]    iop    iop-ext    5_14_2    5_2_39    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}
    ${created}=    Query Entities Via Broker    ${b1_url}    type=${etype}    entityMap=true
    Check Response Status Code    201    ${created.status_code}
    ${map_id}=    Map Id From    ${created.headers}
    ${before}=    GET    url=${b1_url}/entityMaps/${map_id}    expected_status=any
    ${pre}=    Set Variable    ${before.json()['expiresAt']}

    &{headers}=    Create Dictionary    Content-Type=application/json
    ${fragment}=    Evaluate
    ...    {"expiresAt": "2099-01-01T00:00:00Z", "entityMap": {"urn:ngsi-ld:Forged:1": ["@none"]}}
    ${patched}=    PATCH    url=${b1_url}/entityMaps/${map_id}    json=${fragment}
    ...    headers=${headers}    expected_status=any
    Should Be True    ${patched.status_code} in (204, 207)

    ${map}=    GET    url=${b1_url}/entityMaps/${map_id}    expected_status=any
    Check Response Status Code    200    ${map.status_code}
    ${post}=    Set Variable    ${map.json()['expiresAt']}
    Should Be True    '${post}' != '${pre}'    the PATCH must have applied expiresAt
    Should Be True    '${post}' > '${pre}'    the applied expiry extends toward the request
    Should Be True    '${post}' <= '2099-01-01T00:00:00Z'    never past the requested instant
    Should Not Contain    ${map.text}    urn:ngsi-ld:Forged:1

IOP_EXT_MAP_01_06 A Deleted Map Recreates On Use Instead Of Failing
    [Documentation]    5.14.3.4: "The EntityMap shall be removed from the
    ...    broker's internal storage, or memory." 5.7.2.4: a presented map
    ...    location that "cannot be found" makes the broker create a new
    ...    EntityMap — the query still answers.
    [Tags]    iop    iop-ext    5_14_3    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}
    ${created}=    Query Entities Via Broker    ${b1_url}    type=${etype}    entityMap=true
    Check Response Status Code    201    ${created.status_code}
    ${map_ref}=    Get From Dictionary    ${created.headers}    NGSILD-EntityMap
    ${map_id}=    Map Id From    ${created.headers}

    ${deleted}=    DELETE    url=${b1_url}/entityMaps/${map_id}    expected_status=any
    Should Be True    ${deleted.status_code} in (204, 207)
    ${gone}=    GET    url=${b1_url}/entityMaps/${map_id}    expected_status=any
    Check Response Status Code    404    ${gone.status_code}

    ${reused}=    Query Entities Via Broker    ${b1_url}    map_ref=${map_ref}    type=${etype}
    Should Be True    ${reused.status_code} in (200, 201)
    Should Contain    ${reused.text}    ${entity_id}-r

IOP_EXT_MAP_01_07 A Temporal Map Reuses Into Identical Instance Sets
    [Documentation]    5.7.4.4 EntityMap arm: the temporal query creates a
    ...    map; presenting it again serves the same federated entity with
    ...    its instances.
    [Tags]    iop    iop-ext    5_7_4    5_14_5    since_v1.9.1
    ${ops}=    Evaluate    ["retrieveTemporal", "queryTemporal"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${ops}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 41, "observedAt": "2026-05-01T00:00:00Z"}, {"type": "Property", "value": 42, "observedAt": "2026-05-02T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${created}=    Query Temporal With Map    ${EMPTY}    entityMap=true
    Should Be True    ${created.status_code} in (200, 201)
    ${map_ref}=    Get From Dictionary    ${created.headers}    NGSILD-EntityMap
    Should Contain    ${created.text}    ${entity_id}

    ${reused}=    Query Temporal With Map    ${map_ref}
    Check Response Status Code    200    ${reused.status_code}
    Should Contain    ${reused.text}    ${entity_id}
    Should Contain    ${reused.text}    "value":41
    Should Contain    ${reused.text}    "value":42
    Should Not Contain    ${reused.text}    ${entity_id}-x

IOP_EXT_MAP_01_08 The Response Header Carries A Location-Style Map Reference
    [Documentation]    6.4.3.2-2/4.3.6.7: "A specific field pointing to the
    ...    location of a cached EntityMap ... shall be returned within the
    ...    response" — the reference resolves after stripping to its last
    ...    segment, and the stored map's own id is a URI, not a path.
    [Tags]    iop    iop-ext    6_4_3    4_3_6_7    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}
    ${created}=    Query Entities Via Broker    ${b1_url}    type=${etype}    entityMap=true
    Check Response Status Code    201    ${created.status_code}
    ${map_ref}=    Get From Dictionary    ${created.headers}    NGSILD-EntityMap
    ${map_id}=    Map Id From    ${created.headers}

    ${map}=    GET    url=${b1_url}/entityMaps/${map_id}    expected_status=any
    Check Response Status Code    200    ${map.status_code}
    Should Be Equal    ${map.json()['type']}    EntityMap
    Should Contain    ${map.json()['id']}    ${map_id}
    Should Not Contain    ${map.json()['id']}    /entityMaps/


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopMap${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopMap:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopmap-${suffix}

Map Id From
    [Documentation]    6.4.3.2-2: the NGSILD-EntityMap response header is a
    ...    LOCATION — strip to the last path segment before using it as an
    ...    id (a nested path in the URL 405s).
    [Arguments]    ${headers}
    ${ref}=    Get From Dictionary    ${headers}    NGSILD-EntityMap
    ${map_id}=    Evaluate    $ref.rstrip('/').split('/')[-1]
    RETURN    ${map_id}

Query Temporal With Map
    [Arguments]    ${map_ref}    &{params}
    &{headers}=    Create Dictionary
    IF    '${map_ref}' != ''
        Set To Dictionary    ${headers}    NGSILD-EntityMap=${map_ref}
    END
    ${merged}=    Evaluate
    ...    {**{"type": $etype, "timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}, **$params}
    ${response}=    GET    url=${b1_url}/temporal/entities    params=${merged}
    ...    headers=${headers}    expected_status=any
    RETURN    ${response}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    FOR    ${tail}    IN    ${EMPTY}    -l    -r    -hi    -hi2    -lo
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
    END
    Delete Temporal Via Broker    ${b1_url}    ${entity_id}
    Delete Temporal Via Broker    ${b2_url}    ${entity_id}
