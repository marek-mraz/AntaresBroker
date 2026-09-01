*** Settings ***
Documentation       Verify that GeoJSON coordinates outside the WGS84 range are
...                 refused rather than stored.
...
...                 ANTARES DECISION, NOT A CIM 009 REQUIREMENT. 4.7.2 mandates
...                 "the syntax and restrictions mandated by IETF RFC 7946 [8]",
...                 and IETF RFC 7946 4 fixes the coordinate reference system as
...                 "a geographic coordinate reference system, using the World
...                 Geodetic System 1984 [WGS84] datum, with longitude and
...                 latitude units of decimal degrees" — but the RFC states no
...                 numeric bound in so many words, so reading a latitude of 999
...                 as a syntax violation is an interpretation, not a clause.
...
...                 The reason is the storage seam. A latitude outside the range
...                 reaches PostGIS as a `::geography` cast that errors, so one
...                 accepted write would make every later `near` query in that
...                 tenant fail — a client-triggered outage for every other
...                 client of the same tenant. Refusing the write at the boundary
...                 keeps the failure with the request that caused it.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
A Location Outside The WGS84 Range Is Refused
    [Documentation]    A latitude past the pole is not a latitude in decimal
    ...    degrees: the create is a 400 and the entity is not there afterwards.
    [Tags]    e-create    4_7_2
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0, 999]}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}

A Reference Geometry Outside The WGS84 Range Is Refused
    [Documentation]    The same rule on the query side: a geoquery whose
    ...    reference geometry leaves the range is a 400, not a store error.
    [Tags]    e-query    4_10
    ${response}=    Query Entities
    ...    entity_types=Vehicle
    ...    georel=near;maxDistance==1000
    ...    geometry=Point
    ...    coordinates=[181,0]
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
