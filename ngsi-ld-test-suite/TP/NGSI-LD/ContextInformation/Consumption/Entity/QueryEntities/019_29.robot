*** Settings ***
Documentation       Check that the Query Language attribute-path extensions are supported
...                 (CIM 009 clause 4.9): compound-value trailing paths, language filters,
...                 linked-entity subqueries and expandValues.
...
...                 Normative sources: EXAMPLE 9/10 (trailing [path] evaluated as an
...                 ECMA 262 MemberExpression; "If the evaluation of such MemberExpression
...                 does not result in a defined value, the target element shall be
...                 considered as non-existent"), Equal/Unequal languageMap semantics
...                 (color[en], color[*], p.90-91), EXAMPLE 12 (expandValues: query-term
...                 values "expanded against the supplied @context ... prior to executing
...                 the query"), EXAMPLE 13/14 (linked-entity subquery attr{path} with
...                 optional EntityType hints), and "If the target element corresponds to
...                 a Relationship ... any operator different than equal or unequal shall
...                 result in not matching" (p.89).
...
...                 Antares extension TP — the suite exercises none of these constructs.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities
Test Template       Query Entities Expecting Count


*** Variables ***
${first_entity_filename}=       building-query-language-features.jsonld
${second_entity_filename}=      building-query-language-target.jsonld
${first_entity_id}=             urn:ngsi-ld:Building:qlFeatures019x29
${second_entity_id}=            urn:ngsi-ld:Building:qlTarget019x29
${entity_type}=                 Building


*** Test Cases ***    Q_PARAMETER    EXPECTED_STATUS    EXPECTED_COUNT
019_29_01 Compound Value Trailing Path Matches A Subitem
    [Documentation]    4.9 EXAMPLE 9: address[city] addresses the city member of the
    ...    compound value — only the features entity holds city "Berlin"
    [Tags]    e-query    4_9    since_v1.9.1
    address[city]=="Berlin"    200    1

019_29_02 Compound Value Undefined Member Is Non-Existent
    [Documentation]    4.9: "If the evaluation of such MemberExpression does not result
    ...    in a defined value, the target element shall be considered as non-existent" —
    ...    an undefined member must not match anything, even with a matching value
    [Tags]    e-query    4_9    since_v1.9.1
    address[postcode]=="Berlin"    200    0

019_29_03 Existence Check Through The Trailing Path
    [Documentation]    4.9: a Query Term with only an attribute path is an existence
    ...    check; the defined compound member exists on exactly one entity
    [Tags]    e-query    4_9    since_v1.9.1
    address[city]    200    1

019_29_04 Language Filter Matches The Addressed Language Only
    [Documentation]    4.9 Equal p.90: color[en]=="red" matches the value of the "en"
    ...    key of the languageMap
    [Tags]    e-query    4_9    since_v1.9.1
    color[en]=="red"    200    1

019_29_05 Language Filter Does Not Match Another Language's Value
    [Documentation]    4.9 Equal p.90: "rouge" is the fr value — addressing en with it
    ...    must NOT match (a naive any-language match would wrongly return the entity)
    [Tags]    e-query    4_9    since_v1.9.1
    color[en]=="rouge"    200    0

019_29_06 Asterisk Language Filter Matches Any Language
    [Documentation]    4.9 Equal p.90: color[*] — "any match is found in the values of
    ...    the key-value pairs of the languageMap"
    [Tags]    e-query    4_9    since_v1.9.1
    color[*]=="rouge"    200    1

019_29_07 Asterisk Language Filter With Unequal Requires No Match In Any Language
    [Documentation]    4.9 Unequal p.91: color[*]!="red" — "No matching value is found
    ...    in any of the values of the key-value pairs of a languageMap"; the en value
    ...    IS red, so the entity must NOT match
    [Tags]    e-query    4_9    since_v1.9.1
    color[*]!="red"    200    0

019_29_08 Linked Entity Subquery Follows The Relationship
    [Documentation]    4.9 EXAMPLE 13: managedBy{name} makes a sub-query on the entity
    ...    targeted by the managedBy Relationship — the features entity matches through
    ...    the target entity's name
    [Tags]    e-query    4_9    since_v1.9.1
    managedBy{name}=="Pisa Tower"    200    1    join=inline    joinLevel=1

019_29_09 Linked Entity Subquery With A Matching Type Hint
    [Documentation]    4.9 EXAMPLE 14: the EntityType hint restricts the linked lookup;
    ...    the target IS a Building, so the match is preserved
    [Tags]    e-query    4_9    since_v1.9.1
    managedBy{Building:name}=="Pisa Tower"    200    1    join=inline    joinLevel=1

019_29_10 Linked Entity Subquery With A Non-Matching Type Hint
    [Documentation]    4.9 EXAMPLE 14: "only such NGSI-LD Entities need to be
    ...    considered" — a Vehicle hint excludes the Building target, so no match
    [Tags]    e-query    4_9    since_v1.9.1
    managedBy{Vehicle:name}=="Pisa Tower"    200    0    join=inline    joinLevel=1

019_29_11 Ordering Operator On A Relationship Never Matches
    [Documentation]    4.9 p.89: "If the target element corresponds to a Relationship or
    ...    ListRelationship, the combination of such target element with any operator
    ...    different than equal or unequal shall result in not matching"
    [Tags]    e-query    4_9    since_v1.9.1
    managedBy>"urn:a"    200    0

019_29_12 ExpandValues Coerces The Query Term Value
    [Documentation]    4.9 EXAMPLE 12: with expandValues the query-term value is expanded
    ...    against the @context, so the short term matches the expanded vocab URI
    [Tags]    e-query    4_9    since_v1.9.1
    category==commercial    200    1    expandValues=category

019_29_13 Without ExpandValues The Short Term Does Not Match
    [Documentation]    4.9 EXAMPLE 12 (negative): without expandValues the literal string
    ...    is compared against the expanded vocab URI and must NOT match
    [Tags]    e-query    4_9    since_v1.9.1
    category==commercial    200    0


*** Keywords ***
Query Entities Expecting Count
    [Arguments]    ${q}    ${expected_status_code}    ${expected_count}    ${expandValues}=${EMPTY}
    ...    ${join}=${EMPTY}    ${joinLevel}=${EMPTY}
    ${response}=    Query Entities
    ...    entity_types=${entity_type}
    ...    q=${q}
    ...    count=true
    ...    context=${ngsild_test_suite_context}
    ...    expandValues=${expandValues}
    ...    join=${join}
    ...    joinLevel=${joinLevel}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    $expected_count is not None
        Check Response Headers Containing NGSILD-Results-Count Equals To
        ...    ${expected_count}
        ...    ${response.headers}
    END

Setup Initial Entities
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${first_entity_filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${second_entity_filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

Delete Initial Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
