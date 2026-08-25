*** Settings ***
Documentation       Verify 5.7.4.4 S3 geoquery edges on the temporal query:
...                 "select those Entities whose GeoProperty instances meet
...                 the geospatial restrictions imposed by the geoquery ...
...                 checked against the GeoProperty instances that are
...                 within the interval defined by the temporal query" —
...                 across georel variants (near, within, intersects,
...                 disjoint, equals) and a non-default geoproperty. The
...                 pg/timescale CI cells additionally exercise the
...                 store-side windowed geo prefilter these cases feed.
...                 Antares extension TP.

Library             RequestsLibrary
Library             Collections
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create Fixture Entities
Suite Teardown      Delete Fixture Entities


*** Variables ***
${paris}=       urn:ngsi-ld:Vehicle:qg5744-paris
${far}=         urn:ngsi-ld:Vehicle:qg5744-far
${outw}=        urn:ngsi-ld:Vehicle:qg5744-outw
${obs}=         urn:ngsi-ld:Vehicle:qg5744-obs
${window}=      timerel=between&timeAt=2026-03-01T00:00:00Z&endTimeAt=2026-03-02T00:00:00Z


*** Test Cases ***
5744_04_01 Near Max Distance Selects The Close Entity
    [Documentation]    4.10/5.7.4.4 S3: near;maxDistance==2000 around the
    ...    Paris point keeps the co-located entity, the ~550 km one is
    ...    excluded.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Geo    georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=[2.29,48.85]
    Should Contain    ${response.text}    ${paris}
    Should Not Contain    ${response.text}    ${far}

5744_04_02 Out-Of-Window Geometry Never Matches
    [Documentation]    5.7.4.4 S3: the geoquery is checked against the
    ...    GeoProperty instances WITHIN the interval — an entity whose only
    ...    matching geometry is outside the window is excluded.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Geo    georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=[2.29,48.85]
    Should Not Contain    ${response.text}    ${outw}

5744_04_03 Within Polygon
    [Documentation]    4.10: within a polygon around Paris — the far point
    ...    and the out-of-window one stay excluded.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Geo    georel=within&geometry=Polygon&coordinates=[[[2.0,48.5],[2.6,48.5],[2.6,49.2],[2.0,49.2],[2.0,48.5]]]
    Should Contain    ${response.text}    ${paris}
    Should Not Contain    ${response.text}    ${far}
    Should Not Contain    ${response.text}    ${outw}

5744_04_04 Intersects The Query Polygon
    [Documentation]    4.10: intersects with the same polygon behaves like
    ...    within for point targets.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Geo    georel=intersects&geometry=Polygon&coordinates=[[[2.0,48.5],[2.6,48.5],[2.6,49.2],[2.0,49.2],[2.0,48.5]]]
    Should Contain    ${response.text}    ${paris}
    Should Not Contain    ${response.text}    ${far}

5744_04_05 Disjoint Inverts The Selection
    [Documentation]    4.10 disjoint: the far entity matches, the
    ...    co-located one is excluded — and an entity with no windowed
    ...    GeoProperty instance matches nothing, not even disjoint.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Geo    georel=disjoint&geometry=Polygon&coordinates=[[[2.0,48.5],[2.6,48.5],[2.6,49.2],[2.0,49.2],[2.0,48.5]]]
    Should Contain    ${response.text}    ${far}
    Should Not Contain    ${response.text}    ${paris}
    Should Not Contain    ${response.text}    ${outw}

5744_04_06 Equals The Exact Point
    [Documentation]    4.10 equals: only the byte-identical geometry
    ...    matches.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Geo    georel=equals&geometry=Point&coordinates=[2.29,48.85]
    Should Contain    ${response.text}    ${paris}
    Should Not Contain    ${response.text}    ${far}

5744_04_07 Non-Default Geoproperty
    [Documentation]    4.10 geoproperty: the query targets observationSpace
    ...    — only the entity carrying it there matches; entities with a
    ...    matching default location do not.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Geo    georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=[2.29,48.85]&geoproperty=observationSpace
    Should Contain    ${response.text}    ${obs}
    Should Not Contain    ${response.text}    ${paris}
    Should Not Contain    ${response.text}    ${far}


*** Keywords ***
Query Geo
    [Arguments]    ${geo_expr}
    ${response}=    GET
    ...    url=${temporal_api_url}/temporal/entities
    ...    params=type=Vehicle&${geo_expr}&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    RETURN    ${response}

Create Temporal Entity
    [Arguments]    ${payload}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${temporal_api_url}/temporal/entities
    ...    data=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Create Fixture Entities
    Create Temporal Entity    {"id": "${paris}", "type": "Vehicle", "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.29, 48.85]}, "observedAt": "2026-03-01T12:00:00Z"}]}
    Create Temporal Entity    {"id": "${far}", "type": "Vehicle", "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [10.0, 50.0]}, "observedAt": "2026-03-01T12:00:00Z"}]}
    Create Temporal Entity    {"id": "${outw}", "type": "Vehicle", "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.29, 48.85]}, "observedAt": "2026-01-05T00:00:00Z"}]}
    Create Temporal Entity    {"id": "${obs}", "type": "Vehicle", "observationSpace": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.29, 48.85]}, "observedAt": "2026-03-01T12:00:00Z"}]}

Delete Fixture Entities
    FOR    ${id}    IN    ${paris}    ${far}    ${outw}    ${obs}
        ${response}=    DELETE    url=${temporal_api_url}/temporal/entities/${id}    expected_status=any
    END
