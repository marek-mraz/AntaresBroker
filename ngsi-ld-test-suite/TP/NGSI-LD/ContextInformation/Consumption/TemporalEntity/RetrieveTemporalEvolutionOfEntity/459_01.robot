*** Settings ***
Documentation       Verify the 4.5.9 simplified temporal representation for the attribute
...                 types the official TPs skip.
...
...                 Clause 4.5.9: each attribute renders as {"type": <AttrType>,
...                 "<member>": [[<value>, <time>], …]} where the member is "values"
...                 (Property/GeoProperty), "objects" (Relationship), "valueLists"
...                 (ListProperty, bare ordered array — Example 3) or "objectLists"
...                 (ListRelationship, bare ordered array — Example 7); the object
...                 "shall only contain" that member besides its type.
...
...                 Antares extension TP — official temporalValues TPs cover
...                 Property/Relationship (020_10) and the LanguageProperty/
...                 JsonProperty/VocabProperty wraps (020_12/15/16) but not
...                 GeoProperty, ListProperty or ListRelationship.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Temporal Representation Of Entity    ${entity_id}


*** Test Cases ***
459_01 Simplified Temporal Pairs For Geo And List Attribute Types
    [Documentation]    4.5.9: GeoProperty pairs under "values"; ListProperty under
    ...    "valueLists" and ListRelationship under "objectLists" with the BARE
    ...    ordered array as the pair's first element; no members beyond type and
    ...    the per-type member.
    [Tags]    te-retrieve    4_5_9    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}, "observedAt": "2026-08-10T00:00:01Z"}, {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [3.0, 4.0]}, "observedAt": "2026-08-10T00:00:02Z"}], "tyreTreadDepths": {"type": "ListProperty", "valueList": [1.2, 3.4], "observedAt": "2026-08-10T00:00:01Z"}, "route": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Road:1", "urn:ngsi-ld:Road:2"], "observedAt": "2026-08-10T00:00:01Z"}}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${response}=    POST
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${entity_id}
    ...    options=temporalValues
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    ${loc_type}=    Evaluate    $body['location']['type']
    Should Be Equal    ${loc_type}    GeoProperty
    ${loc_pairs}=    Evaluate    $body['location']['values']
    Length Should Be    ${loc_pairs}    2
    ${first_geo}=    Evaluate    $loc_pairs[0][0]['type']
    Should Be Equal    ${first_geo}    Point
    ${list_pair}=    Evaluate    $body['tyreTreadDepths']['valueLists'][0]
    ${bare_list}=    Evaluate    $list_pair[0]
    ${expected_list}=    Evaluate    [1.2, 3.4]
    Should Be Equal    ${bare_list}    ${expected_list}
    ${rel_pair}=    Evaluate    $body['route']['objectLists'][0]
    ${bare_objs}=    Evaluate    $rel_pair[0]
    ${expected_objs}=    Evaluate    ["urn:ngsi-ld:Road:1", "urn:ngsi-ld:Road:2"]
    Should Be Equal    ${bare_objs}    ${expected_objs}
    ${leaks_geo}=    Evaluate    [k for k in $body['location'] if k not in ('type', 'datasetId', 'values')]
    Should Be Empty    ${leaks_geo}
    ${leaks_list}=    Evaluate    [k for k in $body['tyreTreadDepths'] if k not in ('type', 'datasetId', 'valueLists')]
    Should Be Empty    ${leaks_list}
    ${leaks_route}=    Evaluate    [k for k in $body['route'] if k not in ('type', 'datasetId', 'objectLists')]
    Should Be Empty    ${leaks_route}
