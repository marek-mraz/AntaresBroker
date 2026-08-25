*** Settings ***
Documentation       Verify 5.7.1.4 Retrieve Entity edges the official 018 TPs
...                 skip: the ?type selector narrowing the target, the
...                 geometryProperty/Accept coupling, and Linked Entity
...                 projection without join.
...
...                 5.7.1.4: "If geometryProperty parameter is present and
...                 the Accept Header is not set to application/geo+json,
...                 then an error of type BadRequestData shall be raised";
...                 "If projection attributes ... indicate the use of Linked
...                 Entity retrieval and the use of Linked Entity retrieval
...                 is not specified ... BadRequestData"; ResourceNotFound
...                 when no entity "whose id (URI), and where specified
...                 type, is equivalent" exists.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Setup Initial Entity
Suite Teardown      Delete Entity    ${entity_id}


*** Test Cases ***
5711_01_01 Type Selector Mismatch Is ResourceNotFound
    [Documentation]    5.7.1.4: ?type names a type the entity does not have
    ...    → 404 ResourceNotFound; the matching selector still returns 200.
    [Tags]    e-retrieve    5_7_1    4_17    since_v1.9.1
    ${context_link}=    Build Context Link    ${ngsild_test_suite_context}
    &{headers}=    Create Dictionary    Accept=application/json    Link=${context_link}
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}?type=Vehicle
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_RESOURCE_NOT_FOUND}
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}?type=Building
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}

5711_01_02 GeometryProperty Without GeoJSON Accept Is BadRequestData
    [Documentation]    5.7.1.4: geometryProperty with a JSON Accept header
    ...    → 400 BadRequestData; with application/geo+json it succeeds.
    [Tags]    e-retrieve    5_7_1    since_v1.9.1
    &{headers}=    Create Dictionary    Accept=application/ld+json
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}?geometryProperty=location
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}
    &{geo_headers}=    Create Dictionary    Accept=application/geo+json
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}?geometryProperty=location
    ...    headers=${geo_headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}

5711_01_03 Linked Entity Projection Without Join Is BadRequestData
    [Documentation]    5.7.1.4: pick using the {…} Linked Entity selection
    ...    without a join parameter → 400 BadRequestData.
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.9.1
    &{headers}=    Create Dictionary    Accept=application/ld+json
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}?pick=owner{name}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Catenate    ${BUILDING_ID_PREFIX}5711-01-1
    Set Suite Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
